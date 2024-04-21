#![allow(clippy::enum_variant_names)]

use std::{collections::HashMap, env};
use serde::{Serialize, Deserialize};
use actix_web::{post, web::Json, App, Error as ActixError, HttpRequest, HttpResponse, HttpServer, Responder};
use serde_json::{json, Value};

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
struct Intent {
    name: String,
    confirmation_status: String,
    slots: HashMap<String, Value>
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

#[post("/")]
async fn index(req: HttpRequest, info: Json<Value>) -> impl Responder {
    // print all headers and body
    println!("{:?}", req.headers());
    println!("{:?}", info);

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
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let server = HttpServer::new(|| {
        App::new()
            .service(index)
    });

    let port = env::var("PORT").unwrap_or("8080".to_string());
    let addr = format!("127.0.0.1:{port}");
    server.bind(addr).unwrap().run().await
}
