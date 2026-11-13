#[macro_use]
mod lang;
mod about;
mod api;
mod auth;
mod branch;
mod config;
mod files;
mod fs;
mod git;
mod models;
mod pet;
mod runtime;
mod scope;
mod sessions;
mod limits;
mod retry;
mod update;
mod web;
mod ws;

use axum::{
    middleware,
    routing::{delete, get},
    Router,
};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use config::{random_hex, Config};

pub struct AppState {
    /// What may open the API: the token and/or the password. Empty only on a loopback address.
    pub secrets: Vec<String>,
    pub throttle: auth::Throttle,
    pub trust_proxy: bool,
    pub allow_origins: Vec<String>,
    pub sessions_dir: PathBuf,
    pub pi_bin: String,
    pub runtimes: Arc<runtime::Manager>,
    pub sessions: sessions::SessionIndex,
    /// `--root`, resolved: the only folder (and below) this server works in.
    pub root: Option<PathBuf>,
}

/// Browsers on another origin (a Tauri webview, a dev server) need explicit permission.
/// The guard still enforces the origin allow-list and the token; this only answers preflights
/// and adds the response headers.
fn cors(origins: &[String]) -> tower_http::cors::CorsLayer {
    use axum::http::{header, HeaderValue, Method};
    let allowed: Vec<HeaderValue> = origins.iter().filter_map(|o| HeaderValue::from_str(o).ok()).collect();
    tower_http::cors::CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::HeaderName::from_static("x-pi-oven-lang")])
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "pi_oven=info".into()))
        .with_target(false)
        .init();

    let mut cfg = config::parse_args();
    let (given_token, password) = match cfg.secrets() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("pi-oven: {e}");
            std::process::exit(2);
        }
    };
    if let Some(r) = &cfg.root {
        match r.canonicalize() {
            Ok(p) if p.is_dir() => cfg.root = Some(p),
            _ => {
                eprintln!("pi-oven: --root: not a folder: {}", r.display());
                std::process::exit(2);
            }
        }
    }
    if !cfg.no_permission_ext {
        cfg.extension_path = config::install_extension();
    }
    cfg.retry_extension_path = config::install_retry_extension();
    cfg.limits_extension_path = config::install_limits_extension();
    let host = cfg.listen_host();
    let cfg = Arc::new(cfg);
    // Off loopback something has to be asked for: with neither given, a token is made up.
    let generated = given_token.is_none() && password.is_none() && !host.is_loopback();
    let token = given_token.or_else(|| generated.then(|| random_hex(16)));
    let secrets: Vec<String> = token.iter().chain(password.iter()).cloned().collect();

    let runtimes = runtime::Manager::new(cfg.clone());
    let app = Arc::new(AppState {
        secrets,
        throttle: Default::default(),
        trust_proxy: cfg.trust_proxy,
        allow_origins: cfg.allow_origins.clone(),
        sessions_dir: cfg.resolved_sessions_dir(),
        pi_bin: cfg.pi_bin.clone(),
        runtimes: runtimes.clone(),
        sessions: Default::default(),
        root: cfg.root.clone(),
    });

    let router = Router::new()
        .route("/api/info", get(api::info))
        .route("/api/limits", get(limits::get))
        .route("/api/sessions", get(api::sessions).delete(api::delete_session))
        .route("/api/runtimes", get(api::list_runtimes).post(api::create_runtime))
        .route("/api/runtimes/{id}", delete(api::delete_runtime))
        .route("/api/runtimes/{id}/ws", get(ws::handler))
        .route("/api/runtimes/{id}/tasks", get(api::tasks))
        .route("/api/runtimes/{id}/tasks/{task}/output", get(api::task_output))
        .route("/api/runtimes/{id}/restart", axum::routing::post(api::restart_runtime))
        .route("/api/fs", get(api::browse))
        .route("/api/fs/mkdir", axum::routing::post(api::mkdir))
        .route("/api/runtimes/{id}/git", get(git::status))
        .route("/api/runtimes/{id}/git/diff", get(git::diff))
        .route("/api/runtimes/{id}/git/stage", axum::routing::post(git::stage))
        .route("/api/runtimes/{id}/git/commit", axum::routing::post(git::commit))
        .route("/api/runtimes/{id}/files", get(files::list).delete(files::delete))
        .route("/api/runtimes/{id}/files/read", get(files::read))
        .route("/api/runtimes/{id}/files/write", axum::routing::put(files::write).layer(axum::extract::DefaultBodyLimit::max(files::MAX_WRITE + 1_000_000)))
        .route("/api/runtimes/{id}/files/create", axum::routing::post(files::create))
        .route("/api/runtimes/{id}/files/rename", axum::routing::post(files::rename))
        .route("/api/runtimes/{id}/files/stat", axum::routing::post(files::stat))
        .route("/api/models/config", get(models::list).post(models::upsert).delete(models::remove))
        .route("/api/models/probe", axum::routing::post(models::probe))
        .route("/api/sessions/tree", get(branch::tree))
        .route("/api/sessions/state", get(branch::state))
        .route("/api/sessions/tail", get(branch::tail))
        .route("/api/sessions/branch", axum::routing::post(branch::branch))
        .route("/api/sessions/move", axum::routing::post(branch::move_session))
        .route("/api/sessions/rename", axum::routing::post(branch::rename))
        .route("/api/pet/stream", get(api::pet_stream))
        .route("/api/about", get(about::about))
        .route("/api/update", get(update::status).post(update::run))
        .route("/api/retry", get(retry::get_mode).put(retry::set_mode))
        .route("/api/runtimes/{id}/retry", axum::routing::post(retry::act))
        .fallback(web::serve)
        .layer(middleware::from_fn_with_state(app.clone(), auth::guard))
        .layer(middleware::from_fn(lang::layer))
        .layer(cors(&cfg.allow_origins))
        .with_state(app);

    let addr = SocketAddr::new(host, cfg.port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    banner(&cfg, addr, token.as_deref(), generated, password.is_some());

    axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    runtimes.close_all().await;
    Ok(())
}

/// The address other devices would use: whichever interface the system would send traffic out of.
fn lan_ip() -> Option<std::net::IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?; // nothing is sent; this only picks a route
    sock.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified())
}

fn banner(cfg: &Config, addr: SocketAddr, token: Option<&str>, generated: bool, has_password: bool) {
    let frag = token.map(|t| format!("#token={t}")).unwrap_or_default();
    let port = addr.port();
    let ko = lang::cli_korean();
    // labels padded to one column in each language
    let l = |k: &'static str, e: &'static str| if ko { k } else { e };
    println!("pi-oven {} {}", env!("CARGO_PKG_VERSION"), l("시작", "started"));
    if addr.ip().is_unspecified() {
        println!("  {}http://127.0.0.1:{port}/{frag}", l("이 PC     ", "This PC   "));
        match lan_ip() {
            Some(ip) => println!("  {}http://{ip}:{port}/{frag}", l("네트워크  ", "Network   ")),
            None => println!("  {}http://<{}>:{port}/{frag}", l("네트워크  ", "Network   "), l("이 PC의 IP", "this PC's IP")),
        }
    } else {
        println!("  {}http://{addr}/{frag}", l("주소      ", "Address   "));
    }
    if generated {
        println!("  {}{}", l("토큰      ", "Token     "), l(
            "자동으로 만들었습니다(루프백이 아닌 주소에 열었기 때문). 위 주소 끝의 #token=… 을 포함해서 접속하세요.",
            "made for you (the server is open beyond loopback). Open the address above including its #token=… part.",
        ));
    } else if token.is_some() {
        println!("  {}{}", l("토큰      ", "Token     "), l("주소 끝의 #token=… 으로 접속하거나, 접속 화면에 입력하세요.", "open the address with #token=… or type it on the sign-in page."));
    }
    if has_password {
        println!("  {}{}", l("비밀번호  ", "Password  "), l("접속 화면에 입력하세요(--password).", "type it on the sign-in page (--password)."));
    }
    if !addr.ip().is_loopback() {
        println!("  {}{}", l("주의      ", "Note      "), l("HTTP는 암호화되지 않습니다. VPN이나 HTTPS 리버스 프록시 뒤에서 쓰세요.", "HTTP is not encrypted. Use it behind a VPN or an HTTPS reverse proxy."));
    }
    if cfg.trust_proxy {
        println!("  {}{}", l("프록시    ", "Proxy     "), l("X-Forwarded-For를 접속자 구분에 씁니다(--trust-proxy).", "X-Forwarded-For tells clients apart (--trust-proxy)."));
    }
}
