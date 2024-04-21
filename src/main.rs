use std::env;

use actix_web::{post, web::Json, App, Error as ActixError, HttpRequest, HttpServer};
use serde_json::Value;

#[post("/")]
async fn index(req: HttpRequest, info: Json<Value>) -> Result<String, ActixError> {
    // print all headers and body
    println!("{:?}", req.headers());
    println!("{:?}", info);

    Ok(String::from("Hello world!"))
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
