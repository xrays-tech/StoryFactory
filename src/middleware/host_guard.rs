// src/middleware/host_guard.rs — §12.1 Host / Origin / Token 校验
use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::AppError;
use crate::state::AppState;

const LOOPBACK_HOSTS: [&str; 4] = ["127.0.0.1", "localhost", "::1", "[::1]"];

pub async fn host_guard(state: State<AppState>, req: Request, next: Next) -> Response {
    // 非回环绑定：令牌即认证与 CSRF 控制；LAN Origin 不参与白名单（M2）
    if state.is_public {
        let token = request_token(&req);
        if token.is_empty() {
            return AppError::token_required().into_response();
        }
        if token != state.app_token {
            return AppError::token_invalid().into_response();
        }
        let mut res = next.run(req).await;
        res.headers_mut().insert(
            axum::http::header::REFERRER_POLICY,
            axum::http::HeaderValue::from_static("no-referrer"),
        );
        return res;
    }

    // 回环绑定：Host 白名单（防 DNS rebinding）
    let got = header_value(&req, "host").unwrap_or_default();
    if !host_allowed(got, &state.bind_host, state.bind_port) {
        tracing::warn!(host = %got, "HostGuard: rejected Host header");
        return AppError::host_forbidden().into_response();
    }

    // 非 GET/HEAD 且带 Origin 时必须同源
    if req.method() != Method::GET && req.method() != Method::HEAD {
        if let Some(origin) = header_value(&req, "origin") {
            if !origin_allowed(&state.origins, origin) {
                tracing::warn!(origin = %origin, "OriginGuard: rejected Origin header");
                return AppError::origin_forbidden().into_response();
            }
        }
    }

    next.run(req).await
}

fn header_value<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers().get(name).and_then(|h| h.to_str().ok())
}

/// Host 头形如 `host:port`；端口必须等于 bind 端口，host 在回环白名单或等于 bind host。
pub fn host_allowed(host_header: &str, bind_host: &str, bind_port: u16) -> bool {
    let (host, port) = match host_header.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()),
        None => (host_header, None),
    };
    if port != Some(bind_port) {
        return false;
    }
    LOOPBACK_HOSTS.contains(&host) || host == bind_host
}

/// Origin 必须精确匹配白名单（scheme + host + port）。
pub fn origin_allowed(allowed: &[String], origin: &str) -> bool {
    allowed.iter().any(|a| a == origin)
}

/// 从 query 中提取 `token=`（SSE 无法设置请求头，D-26）。
pub fn token_from_query(query: &str) -> String {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
        .unwrap_or("")
        .to_string()
}

fn request_token(req: &Request) -> String {
    if let Some(v) = header_value(req, "x-app-token") {
        if !v.is_empty() {
            return v.to_string();
        }
    }
    req.uri().query().map(token_from_query).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_allowlist() {
        assert!(host_allowed("127.0.0.1:8080", "127.0.0.1", 8080));
        assert!(host_allowed("localhost:8080", "127.0.0.1", 8080));
        assert!(host_allowed("[::1]:8080", "127.0.0.1", 8080));
        assert!(!host_allowed("evil.com:8080", "127.0.0.1", 8080));
        assert!(!host_allowed("127.0.0.1:9090", "127.0.0.1", 8080));
        assert!(!host_allowed("127.0.0.1", "127.0.0.1", 8080));
    }

    #[test]
    fn query_token() {
        assert_eq!(token_from_query("token=abc"), "abc");
        assert_eq!(token_from_query("x=1&token=abc"), "abc");
        assert_eq!(token_from_query("x=1"), "");
    }
}
