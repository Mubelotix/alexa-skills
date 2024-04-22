#![allow(clippy::enum_variant_names)]
#![recursion_limit = "256"]

use std::{collections::HashMap, env, time::Duration};
use serde::Deserialize;
use actix_web::{get, post, rt::spawn, web::{Data, Json}, App, HttpRequest, HttpResponse, HttpServer, Responder};
use serde_json::{json, Value};
use string_tools::{get_all_before_strict, get_all_between_strict};
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
    Simple { resolutions: Value, value: String },
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

struct AppState {
    default_departures: RwLock<HashMap<String, String>>,
    default_destinations: RwLock<HashMap<String, String>>,
}

async fn get_route(stop_id: usize, line_id: usize, sens: usize) -> Result<Option<usize>, String> {
    let url = "https://www.reseau-astuce.fr/fr/horaires-a-larret/28/StopTimeTable/NextDeparture";
    let response = reqwest::Client::new().post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.3")
        .body(format!("destinations=%7B%221%22%3A%22%22%7D&stopId={stop_id}&lineId={line_id}&sens={sens}"))
        .send().await.map_err(|e| format!("Erreur lors de la requête: {e}"))?;
    let response = response.text().await.map_err(|e| format!("Erreur lors de la lecture de la réponse: {e}"))?;
    if response.contains("Pas de prochain") {
        return Ok(None);
    }
    let response = get_all_before_strict(&response, "<abbr title=\"minutes\">")
        .and_then(|s| s.rfind(|c: char| c.is_ascii_digit()).map(|i| &s[i..]))
        .ok_or(String::from("Horaires indisponibles"))?;
    let time = response.parse::<usize>().map_err(|_| String::from("Horaires invalides."))?;

    Ok(Some(time))
}

async fn handle_intent(session: Session, intent: Intent, data: Data<AppState>) -> Result<String, String> {
    match intent.name.as_str() {
        "SetDefaultDeparture" => {
            let departure = intent.slots.get("depart").and_then(|d| d.value.as_ref()).ok_or(String::from("Lieu de départ manquant."))?;

            data.default_departures.write().await.insert(session.user.user_id.clone(), departure.clone());

            Ok(format!("Votre lieu de départ par défaut est maintenant {departure}. Vous ne devrez plus le préciser à chaque fois."))
        },
        "SetDefaultDestination" => {
            let destination = intent.slots.get("destination").and_then(|d| d.value.as_ref()).ok_or(String::from("Lieu de destination manquant."))?;

            data.default_destinations.write().await.insert(session.user.user_id.clone(), destination.clone());

            Ok(format!("Votre lieu de destination par défaut est maintenant {destination}. Vous ne devrez plus le préciser à chaque fois."))
        },
        "AskDefaults" => {
            let departure = data.default_departures.read().await.get(&session.user.user_id).cloned();
            let destination = data.default_destinations.read().await.get(&session.user.user_id).cloned();

            match (departure, destination) {
                (Some(departure), Some(destination)) => Ok(format!("Votre lieu de départ par défaut est {departure} et votre lieu de destination par défaut est {destination}.")),
                (Some(departure), None) => Ok(format!("Votre lieu de départ par défaut est {departure} mais vous n'avez pas de lieu de destination par défaut.")),
                (None, Some(destination)) => Ok(format!("Votre lieu de destination par défaut est {destination} mais vous n'avez pas de lieu de départ par défaut.")),
                (None, None) => Ok(String::from("Vous n'avez ni lieu de départ ni lieu de destination par défaut."))
            }
        }
        "DeleteData" => {
            data.default_departures.write().await.remove(&session.user.user_id);
            data.default_destinations.write().await.remove(&session.user.user_id);

            Ok(String::from("Toutes vos données ont été supprimées."))
        }
        "LeaveTimeIntent" => {
            let departure = match intent.slots.get("depart").and_then(|d| d.value.as_ref()) {
                Some(departure) => departure.to_owned(),
                None => data.default_departures.read().await.get(&session.user.user_id).ok_or(String::from("Lieu de départ manquant."))?.to_owned()
            };
            let destination = match intent.slots.get("destination").and_then(|d| d.value.as_ref()) {
                Some(destination) => destination.to_owned(),
                None => data.default_destinations.read().await.get(&session.user.user_id).ok_or(String::from("Lieu de destination manquant."))?.to_owned()
            };

            let time_left = get_route(202300, 327, 1).await?;

            match time_left {
                Some(time) => Ok(format!("Le prochain tram pour aller de {departure} à {destination} part dans {time} minutes.")),
                None => Ok(format!("Il n'y a pas de tram pour aller de {departure} à {destination} dans les prochaines heures."))
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

    let alexa_request: AlexaRequest = match serde_json::from_value(info.0.clone()) {
        Ok(request) => request,
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
    println!("{:#?}", alexa_request);

    match alexa_request.request {
        Request::LaunchRequest { .. } => HttpResponse::Ok().json(json!(
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
        )),
        Request::IntentRequest { intent, .. } => match handle_intent(alexa_request.session, intent, data).await {
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
        Request::SessionEndedRequest { .. } => HttpResponse::Ok().body(()),
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
        Ok(data) => {
            let data: Value = serde_json::from_slice(&data).unwrap();
            let default_departures = data["default_departures"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
            let default_destinations = data["default_destinations"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
            Data::new(AppState {
                default_departures: RwLock::new(default_departures),
                default_destinations: RwLock::new(default_destinations),
            })
        },
        Err(_) => {
            println!("No data could be restored.");
            Data::new(AppState {
                default_departures: RwLock::new(HashMap::new()),
                default_destinations: RwLock::new(HashMap::new()),
            })
        }
    };

    // Save data periodically
    let data2 = data.clone();
    spawn(async move {
        let mut previous_len = 0;
        loop {
            let default_departures = data2.default_departures.read().await.clone();
            let default_destinations = data2.default_destinations.read().await.clone();

            let data = json!({
                "default_departures": default_departures,
                "default_destinations": default_destinations
            });
            let data = serde_json::to_string(&data).unwrap();
            if data.len() != previous_len && data.len() <= 50_000_000 {
                previous_len = data.len();
                if let Err(e) = fs::write("data.json", data).await {
                    println!("Error: {:?}", e)
                }
            }

            sleep(Duration::from_secs(3*60)).await;
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
