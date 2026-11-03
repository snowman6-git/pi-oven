//! Korean or English for what the server itself says: API errors follow the language the web UI is in (it sends
//! `X-Pi-Oven-Lang`), the command line follows the system locale (`LC_ALL`, `LC_MESSAGES`, `LANG`). Anything else is
//! English.

use axum::{extract::Request, middleware::Next, response::Response};

tokio::task_local! {
    static KO: bool;
}

/// The terminal's locale is Korean.
pub fn cli_korean() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("ko"))
}

/// Korean for this request (or, outside one, for the terminal).
pub fn ko() -> bool {
    KO.try_with(|k| *k).unwrap_or_else(|_| cli_korean())
}

/// `tr!("한국어", "English")`, either side an expression of the same type (a literal or a `format!`).
#[macro_export]
macro_rules! tr {
    ($ko:expr, $en:expr $(,)?) => {
        if $crate::lang::ko() { $ko } else { $en }
    };
}

/// Middleware: the request's language, from the web UI's header, else the browser's `Accept-Language`.
pub async fn layer(req: Request, next: Next) -> Response {
    let ko = request_is_korean(&req);
    KO.scope(ko, next.run(req)).await
}

fn request_is_korean(req: &Request) -> bool {
    let header = |name: &str| req.headers().get(name).and_then(|v| v.to_str().ok()).map(|v| v.trim().to_ascii_lowercase());
    header("x-pi-oven-lang").or_else(|| header("accept-language")).is_some_and(|v| v.starts_with("ko"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_request_scope_wins_over_the_terminal() {
        assert!(KO.scope(true, async { ko() }).await);
        assert!(!KO.scope(false, async { ko() }).await);
        assert_eq!(KO.scope(true, async { tr!("가", "a") }).await, "가");
    }
}
