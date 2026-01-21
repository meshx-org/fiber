use wstd::http::{Body, Request, Response};

mod handlers;
mod session;
mod types;

#[wstd::http_server]
async fn main(req: Request<Body>) -> Result<Response<Body>, wstd::http::Error> {
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let method = req.method();

    match (method.as_str(), path) {
        ("GET", "/.well-known/jmap") => handlers::session(req).await,
        ("POST", "/jmap") => handlers::jmap_api(req).await,
        ("GET", path) if path.starts_with("/download/") => handlers::download(req).await,
        ("POST", path) if path.starts_with("/upload/") => handlers::upload(req).await,
        _ => handlers::not_found(req).await,
    }
}
