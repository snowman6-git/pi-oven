use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist/"]
#[allow_missing = true]
struct Assets;

pub async fn serve(uri: Uri, headers: HeaderMap) -> Response {
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

    // The build puts a `.br` and a `.gz` beside every text file worth it (web/vite.config.ts): the smallest the browser takes.
    let packed = encodings(&headers).into_iter().find_map(|(enc, ext)| Assets::get(&format!("{name}{ext}")).map(|f| (enc, f)));
    let (encoding, data) = match packed {
        Some((enc, f)) => (Some(enc), f.data),
        None => (None, file.data),
    };

    let mime = mime_guess::from_path(name).first_or_octet_stream();
    let cache = if name.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    let mut res = (
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (header::CACHE_CONTROL, cache.to_string()),
            // no other site may put this page in a frame (clicks could be steered onto it)
            (header::X_FRAME_OPTIONS, "DENY".to_owned()),
            (header::CONTENT_SECURITY_POLICY, "frame-ancestors 'none'".to_owned()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
            // a cache in between must not hand one browser's encoding to another
            (header::VARY, "Accept-Encoding".to_owned()),
        ],
        data,
    )
        .into_response();
    if let Some(enc) = encoding {
        res.headers_mut().insert(header::CONTENT_ENCODING, HeaderValue::from_static(enc));
    }
    res
}

/// The encodings the request takes of the ones the build has, best first (brotli is 13-20% smaller than gzip here),
/// with the ending of the file that holds them.
fn encodings(headers: &HeaderMap) -> Vec<(&'static str, &'static str)> {
    let asked: Vec<(String, bool)> = headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|part| {
            let mut it = part.split(';');
            let name = it.next().unwrap_or("").trim().to_ascii_lowercase();
            // `q=0` (0.0, 0.000) says "not this one"
            let refused = it.any(|p| p.trim().strip_prefix("q=").and_then(|q| q.trim().parse::<f32>().ok()).is_some_and(|q| q <= 0.0));
            (name, !refused)
        })
        .collect();
    // a name spelled out wins over `*`
    let takes = |enc: &str| asked.iter().find(|(n, _)| n == enc).or_else(|| asked.iter().find(|(n, _)| n == "*")).is_some_and(|(_, ok)| *ok);
    [("br", ".br"), ("gzip", ".gz")].into_iter().filter(|(enc, _)| takes(enc)).collect()
}

fn missing() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "The web app is not built yet. Run `npm install && npm run build` in web/, then rebuild the server.",
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picked(v: &str) -> Vec<&'static str> {
        let mut h = HeaderMap::new();
        h.insert(header::ACCEPT_ENCODING, v.parse().unwrap());
        encodings(&h).into_iter().map(|(e, _)| e).collect()
    }

    #[test]
    fn the_encoding_is_one_the_browser_takes_brotli_first() {
        assert_eq!(picked("gzip, deflate, br, zstd"), ["br", "gzip"]);
        assert_eq!(picked("gzip, deflate"), ["gzip"]);
        assert_eq!(picked("BR;q=0.5 , gzip;q=0"), ["br"]);
        assert_eq!(picked("*;q=0.1, br;q=0.0"), ["gzip"]);
        assert!(picked("identity").is_empty() && picked("").is_empty());
        assert!(encodings(&HeaderMap::new()).is_empty());
    }
}
