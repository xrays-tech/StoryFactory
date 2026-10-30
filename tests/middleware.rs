// tests/middleware.rs — Host/Origin/Token 与日志脱敏辅助函数（§12.1/§12.4）
use story_architect::middleware::host_guard::{host_allowed, origin_allowed, token_from_query};
use story_architect::middleware::request_log::is_sensitive_path;

#[test]
fn token_query_parsing() {
    assert_eq!(token_from_query("token=abc123"), "abc123");
    assert_eq!(token_from_query("x=1&token=zz9"), "zz9");
    assert_eq!(token_from_query("foo=bar"), "");
}

#[test]
fn origin_allow_list() {
    let allowed = vec!["http://127.0.0.1:8080".to_string()];
    assert!(origin_allowed(&allowed, "http://127.0.0.1:8080"));
    assert!(!origin_allowed(&allowed, "http://evil.example"));
}

#[test]
fn host_allow_list() {
    assert!(host_allowed("127.0.0.1:8080", "127.0.0.1", 8080));
    assert!(host_allowed("localhost:8080", "127.0.0.1", 8080));
    assert!(host_allowed("[::1]:8080", "127.0.0.1", 8080));
    assert!(!host_allowed("evil.example:8080", "127.0.0.1", 8080));
    assert!(!host_allowed("127.0.0.1:9090", "127.0.0.1", 8080));
}

#[test]
fn log_query_masking() {
    assert!(is_sensitive_path("/api/v1/sandbox-sessions/x/events"));
    assert!(!is_sensitive_path("/api/v1/novels"));
}
