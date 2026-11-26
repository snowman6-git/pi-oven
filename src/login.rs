//! Sign-in: the configured secret is traded once for a session token (`auth.rs` keeps only its hash), and the page
//! keeps that token instead of the secret. Also the WebSocket ticket and the session list.

use axum::{
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::json;
use std::{net::SocketAddr, sync::Arc};

use crate::{
    auth::{client_ip, constant_time_eq, Current},
    AppState,
};

#[derive(Deserialize)]
pub struct LoginBody {
    secret: String,
    device: Option<String>,
}

fn issued(app: &AppState, device: Option<String>) -> Response {
    let (token, id) = app.auth.create(device.as_deref().unwrap_or(""));
    Json(json!({ "token": token, "id": id })).into_response()
}

pub async fn login(State(app): State<Arc<AppState>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap, Json(body): Json<LoginBody>) -> Response {
    if app.secrets.is_empty() {
        return Json(json!({ "token": null, "id": null })).into_response();
    }
    let ip = client_ip(&headers, peer, app.trust_proxy);
    // every secret is compared, so how long it takes does not say which one matched
    if !app.secrets.iter().fold(false, |acc, s| constant_time_eq(&body.secret, s) | acc) {
        app.throttle.failed(ip);
        return (StatusCode::UNAUTHORIZED, "wrong token or password").into_response();
    }
    app.throttle.succeeded(ip);
    issued(&app, body.device)
}

pub async fn logout(State(app): State<Arc<AppState>>, current: Option<Extension<Current>>) -> StatusCode {
    if let Some(Extension(Current(id))) = current {
        app.auth.remove(&id);
    }
    StatusCode::NO_CONTENT
}

pub async fn ticket(State(app): State<Arc<AppState>>, current: Option<Extension<Current>>) -> Response {
    let of = current.map(|c| c.0 .0).unwrap_or_default();
    Json(json!({ "ticket": app.auth.new_ticket(&of) })).into_response()
}

pub async fn sessions(State(app): State<Arc<AppState>>, current: Option<Extension<Current>>) -> Response {
    let mut res = Json(json!({ "current": current.map(|c| c.0 .0), "sessions": app.auth.list() })).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

pub async fn remove(State(app): State<Arc<AppState>>, Path(id): Path<String>) -> StatusCode {
    if app.auth.remove(&id) { StatusCode::NO_CONTENT } else { StatusCode::NOT_FOUND }
}
