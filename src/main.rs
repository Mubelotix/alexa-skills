use std::env;

use actix_web::{post, web::Json, App, Error as ActixError, HttpRequest, HttpResponse, HttpServer, Responder};
use serde_json::{json, Value};

#[post("/")]
async fn index(req: HttpRequest, info: Json<Value>) -> impl Responder {
    // print all headers and body
    println!("{:?}", req.headers());
    println!("{:?}", info);

    HttpResponse::Ok().json(json!(
        {
            "version": "1.0",
            "response": {
                "outputSpeech": {
                    "type": "PlainText",
                    "text": "Test réussi !"
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
