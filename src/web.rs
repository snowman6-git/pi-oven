use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist/"]
#[allow_missing = true]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let wanted = if path.is_empty() { "index.html" } else { path };

    // Unknown paths without an extension are client-side routes.
    let (name, file) = match Assets::get(wanted) {
        Some(f) => (wanted, f),
        None if wanted == "index.html" || !wanted.contains('.') => match Assets::get("index.html") {
            Some(f) => ("index.html", f),
            None => return missing(),
        },
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let mime = mime_guess::from_path(name).first_or_octet_stream();
    let cache = if name.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    ([(header::CONTENT_TYPE, mime.to_string()), (header::CACHE_CONTROL, cache.to_string())], file.data).into_response()
}

fn missing() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "The web app is not built yet. Run `npm install && npm run build` in web/, then rebuild the server.",
    )
        .into_response()
}
