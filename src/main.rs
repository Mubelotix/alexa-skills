#![allow(clippy::enum_variant_names)]

use std::{collections::HashMap, env, hash::Hash, sync::RwLock};
use serde::{Serialize, Deserialize};
use actix_web::{post, web::{Data, Json}, App, Error as ActixError, HttpRequest, HttpResponse, HttpServer, Responder};
use serde_json::{json, Value};
use tokio::sync::Mutex;

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

async fn handle_intent(session: Session, intent: Intent, data: Data<AppState>) -> Result<String, String> {
    match intent.name.as_str() {
        "SetDefaultDeparture" => {
            let departure = intent.slots.get("depart").ok_or(String::from("Lieu de départ manquant."))?;
            let departure = departure.value.as_ref().ok_or(String::from("Lieu de départ manquant."))?;

            data.default_departures.write().unwrap().insert(session.user.user_id.clone(), departure.clone());

            Ok(format!("Votre lieu de départ par défaut est maintenant {departure}. Vous ne devrez plus le préciser à chaque fois."))
        },
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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let data = Data::new(AppState {
        default_departures: RwLock::new(HashMap::new()),
        default_destinations: RwLock::new(HashMap::new()),
    });

    let server = HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .service(index)
    });

    let port = env::var("PORT").unwrap_or("8080".to_string());
    let addr = format!("127.0.0.1:{port}");
    server.bind(addr).unwrap().run().await
}
