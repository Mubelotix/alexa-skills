#![allow(clippy::enum_variant_names)]
#![recursion_limit = "256"]

mod routing;
pub use routing::*;

use std::{collections::HashMap, env, hash::{DefaultHasher, Hash, Hasher}, time::Duration};
use serde::{Serialize, Deserialize};
use actix_web::{get, post, rt::spawn, web::{Data, Json}, App, HttpRequest, HttpResponse, HttpServer, Responder};
use serde_json::{json, Value};
use tokio::{sync::RwLock, time::sleep, fs};

// Best doc : https://developer.amazon.com/en-US/docs/alexa/custom-skills/request-and-response-json-reference.html#request-format

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Application {
    application_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct User {
    user_id: String,
    access_token: Option<String>
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Session {
    new: bool,
    session_id: String,
    attributes: HashMap<String, String>,
    application: Application,
    user: User,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Slot {
    confirmation_status: String,
    name: String,
    resolutions: Option<Value>,
    slot_value: Option<SlotValue>,
    value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all_fields = "camelCase")]
#[serde(tag = "type")]
enum SlotValue {
    Simple { resolutions: Option<Value>, value: String },
    List { values: Vec<SlotValue> },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Intent {
    name: String,
    confirmation_status: String,
    #[serde(default)]
    slots: HashMap<String, Slot>
}


// Best doc : https://developer.amazon.com/en-US/docs/alexa/custom-skills/request-types-reference.html#canfulfillintentrequest
#[derive(Debug, Deserialize)]
#[serde(rename_all_fields = "camelCase")]
#[serde(tag = "type")]
enum Request {
    LaunchRequest { request_id: String, timestamp: String, locale: String },
    IntentRequest { request_id: String, timestamp: String, dialog_state: Option<String>, intent: Intent, locale: String },
    SessionEndedRequest { request_id: String, timestamp: String, reason: String, locale: String, error: Option<Value> },
}

#[derive(Debug, Deserialize)]
struct AlexaRequest {
    version: String,
    session: Session,
    context: Value,
    request: Request,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InnerAppState {
    default_departures: HashMap<String, (usize, usize)>,
    default_destinations: HashMap<String, usize>,
}

type AppState = RwLock<InnerAppState>;

async fn handle_intent(session: Session, intent: Intent, data: Data<AppState>) -> Result<String, String> {
    match intent.name.as_str() {
        "SetDefaultDeparture" => {
            let departure = intent.slots.get("depart")
                .and_then(|d| d.value.as_ref())
                .ok_or(String::from("Lieu de départ manquant."))?;
            let stop_id = get_stop_id(departure).ok_or(String::from("Lieu de départ inconnu."))?;

            let time = intent.slots.get("temps")
                .and_then(|t| t.value.as_ref())
                .and_then(|t| iso8601::duration(t).ok())
                .map(|t| match t {
                    iso8601::Duration::YMDHMS { year, month, day, hour, minute, second, millisecond } => year * 365 * 24 * 60 + month * 30 * 24 * 60 + day * 24 * 60 + hour * 60 + minute + second / 60 + millisecond / 60000,
                    iso8601::Duration::Weeks(weeks) => weeks * 7 * 24 * 60,
                })
                .map(|t| t as usize)
                .unwrap_or(0);

            data.write().await.default_departures.insert(session.user.user_id.clone(), (stop_id, time));

            Ok(match time {
                0 => format!("Votre lieu de départ par défaut est maintenant {departure}."),
                time => format!("Votre lieu de départ par défaut est maintenant à {time} minutes de {departure}.")
            })
        },
        "SetDefaultDestination" => {
            let destination = intent.slots.get("destination")
                .and_then(|d| d.value.as_ref())
                .ok_or(String::from("Lieu de destination manquant."))?;
            let stop_id = get_stop_id(destination).ok_or(String::from("Lieu de destination inconnu."))?;

            data.write().await.default_destinations.insert(session.user.user_id.clone(), stop_id);

            Ok(format!("Votre lieu de destination par défaut est maintenant {destination}. Vous ne devrez plus le préciser à chaque fois."))
        },
        "AskDefaults" => {
            let departure = data.read().await.default_departures
                .get(&session.user.user_id)
                .cloned()
                .map(|(departure, time)| (STOPS.iter().find(|(_, stop_id, _)| *stop_id == departure).unwrap().0[0].clone(), time));
            let destination = data.read().await.default_destinations
                .get(&session.user.user_id)
                .cloned()
                .map(|destination| STOPS.iter().find(|(_, stop_id, _)| *stop_id == destination).unwrap().0[0].clone());

            match (departure, destination) {
                (Some((departure, 0)), Some(destination)) => Ok(format!("Votre lieu de départ par défaut est {departure} et votre lieu de destination par défaut est {destination}.")),
                (Some((departure, time)), Some(destination)) => Ok(format!("Votre lieu de départ par défaut est à {time} minutes de {departure} et votre lieu de destination par défaut est {destination}.")),
                (Some((departure, 0)), None) => Ok(format!("Votre lieu de départ par défaut est {departure} mais vous n'avez pas de lieu de destination par défaut.")),
                (Some((departure, time)), None) => Ok(format!("Votre lieu de départ par défaut est à {time} minutes de {departure} mais vous n'avez pas de lieu de destination par défaut.")),
                (None, Some(destination)) => Ok(format!("Votre lieu de destination par défaut est {destination} mais vous n'avez pas de lieu de départ par défaut.")),
                (None, None) => Ok(String::from("Vous n'avez ni lieu de départ ni lieu de destination par défaut."))
            }
        }
        "DeleteData" => {
            data.write().await.default_departures.remove(&session.user.user_id);
            data.write().await.default_destinations.remove(&session.user.user_id);

            Ok(String::from("Toutes vos données ont été supprimées."))
        }
        "LeaveTimeIntent" => {
            let (from_stop_id, time) = match intent.slots.get("depart").and_then(|d| d.value.as_ref()) {
                Some(departure) => (get_stop_id(departure).ok_or(String::from("Lieu de départ inconnu."))?, 0),
                None => data.read().await.default_departures.get(&session.user.user_id).ok_or(String::from("Lieu de départ manquant."))?.to_owned()
            };
            let departure = STOPS.iter().find(|(_, stop_id, _)| *stop_id == from_stop_id).unwrap().0[0].clone();
            let to_stop_id = match intent.slots.get("destination").and_then(|d| d.value.as_ref()) {
                Some(destination) => get_stop_id(destination).ok_or(String::from("Lieu de destination inconnu."))?,
                None => data.read().await.default_destinations.get(&session.user.user_id).ok_or(String::from("Lieu de destination manquant."))?.to_owned()
            };
            let destination = STOPS.iter().find(|(_, stop_id, _)| *stop_id == to_stop_id).unwrap().0[0].clone();

            let sens = get_sens(from_stop_id, to_stop_id);
            let mut results = get_time_left(from_stop_id, 327, sens).await?;
            results.retain(|r| *r > time);
            let time_left = results.first().cloned().map(|t| t-time);
            match time_left {
                Some(time_left) if time != 0 => Ok(format!("Vous avez {time_left} minutes avant de devoir partir pour prendre le prochain tramway à {departure}. Le tramway partira pour {destination} dans {} minutes.", time_left+time)),
                Some(time_left) => Ok(format!("Vous avez {time_left} minutes pour prendre le prochain tramway à {departure} se rendant à {destination}.")),
                None => Ok(format!("Il n'y a pas de tramway pour aller de {departure} à {destination} dans les prochaines heures."))
            }
        }
        _ => Err(String::from("Désolé, je ne suis pas capable de traiter cette requête"))
    }
}

#[post("/")]
async fn index(req: HttpRequest, info: Json<Value>, data: Data<AppState>) -> impl Responder {
    // print all headers and body
    //println!("{:?}", req.headers());
    //println!("{:?}", info);

    let AlexaRequest { version, session, context, request } = match serde_json::from_value(info.0.clone()) {
        Ok(request) => dbg!(request),
        Err(e) => {
            println!("Error: {:?}", e);
            return HttpResponse::Ok().json(json!(
                {
                    "version": "1.0",
                    "response": {
                        "outputSpeech": {
                            "type": "PlainText",
                            "text": "Désolé, une erreur est survenue lors de la lecture de votre requête"
                        },
                        "shouldEndSession": true
                    }
                }
            ))
        }
    };

    match request {
        Request::LaunchRequest { .. } => {
            let departure = data.read().await.default_departures.get(&session.user.user_id).cloned();
            let destination = data.read().await.default_destinations.get(&session.user.user_id).cloned();

            if let (Some((from_stop_id, time)), Some(to_stop_id)) = (departure, destination) {
                let departure = STOPS.iter().find(|(_, stop_id, _)| *stop_id == from_stop_id).unwrap().0[0].clone();
                let destination = STOPS.iter().find(|(_, stop_id, _)| *stop_id == to_stop_id).unwrap().0[0].clone();
                let sens = get_sens(from_stop_id, to_stop_id);
                if let Ok(mut results) = get_time_left(from_stop_id, 327, sens).await {
                    results.retain(|r| *r > time);
                    if let Some(time_left) = results.first().cloned().map(|t| t-time) {
                        return HttpResponse::Ok().json(json!(
                            {
                                "version": "1.0",
                                "response": {
                                    "outputSpeech": {
                                        "type": "PlainText",
                                        "text": match time != 0 {
                                            true => format!("Vous avez {time_left} minutes avant de devoir partir pour prendre le prochain tramway à {departure}. Le tramway partira pour {destination} dans {} minutes.", time_left+time),
                                            false => format!("Vous avez {time_left} minutes pour prendre le prochain tramway à {departure} se rendant à {destination}.")
                                        }
                                    },
                                    "shouldEndSession": false
                                }
                            }
                        ))
                    }
                }
            }

            HttpResponse::Ok().json(json!(
                {
                    "version": "1.0",
                    "response": {
                        "outputSpeech": {
                            "type": "PlainText",
                            "text": "Où voulez-vous aller ?"
                        },
                        "reprompt": {
                            "outputSpeech": {
                                "type": "PlainText",
                                "text": "Où voulez-vous aller ?"
                            }
                        },
                        "shouldEndSession": false
                    }
                }
            ))
        },
        Request::IntentRequest { intent, .. } => match handle_intent(session, intent, data).await {
            Ok(response) | Err(response) => HttpResponse::Ok().json(json!(
                {
                    "version": "1.0",
                    "response": {
                        "outputSpeech": {
                            "type": "PlainText",
                            "text": response
                        },
                        "shouldEndSession": false
                    }
                }
            )),
        },
        Request::SessionEndedRequest { .. } => HttpResponse::Ok().json(json!(
            {
                "version": "1.0",
                "response": {
                    "shouldEndSession": true
                }
            }
        )),
    }
}

#[get("privacy")]
async fn privacy() -> impl Responder {
    HttpResponse::Ok().append_header(("Content-Type", "text/html")).body(include_str!("../privacy.html"))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load data from file
    let data = match fs::read("data.json").await {
        Ok(data) => match serde_json::from_slice(&data) {
            Ok(data) => Data::new(AppState::new(data)),
            Err(e) => {
                println!("Data could not be restored: {:?}", e);
                Data::new(RwLock::new(InnerAppState {
                    default_departures: HashMap::new(),
                    default_destinations: HashMap::new(),
                }))
            }
        }
        Err(_) => {
            println!("No data could be restored.");
            Data::new(RwLock::new(InnerAppState {
                default_departures: HashMap::new(),
                default_destinations: HashMap::new(),
            }))
        }
    };

    // Save data periodically
    let data2 = data.clone();
    spawn(async move {
        let mut previous_hash = 0;
        loop {
            sleep(Duration::from_secs(3*60)).await;
            
            let data = data2.read().await.clone();
            let data = serde_json::to_string(&data).unwrap();
            let mut hasher = DefaultHasher::new();
            data.hash(&mut hasher);
            let hash = hasher.finish();
            if hash != previous_hash && data.len() <= 50_000_000 {
                previous_hash = hash;
                if let Err(e) = fs::write("data.json", data).await {
                    println!("Error: {:?}", e)
                }
            }
        }
    });

    let server = HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .service(index)
            .service(privacy)
    });

    let port = env::var("PORT").unwrap_or("8080".to_string());
    let addr = format!("127.0.0.1:{port}");
    server.bind(addr).unwrap().run().await
}
