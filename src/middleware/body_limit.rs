// src/middleware/body_limit.rs — body size layers（§5.4）
use tower_http::limit::RequestBodyLimitLayer;

pub const DEFAULT_BODY_LIMIT_BYTES: usize = 2 * 1024 * 1024;
pub const DEFAULT_IMPORT_BODY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

pub fn global_layer(limit_bytes: usize) -> RequestBodyLimitLayer {
    RequestBodyLimitLayer::new(limit_bytes)
}

pub fn import_layer(limit_bytes: usize) -> RequestBodyLimitLayer {
    RequestBodyLimitLayer::new(limit_bytes)
}
