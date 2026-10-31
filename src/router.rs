// src/router.rs — 路由与中间件链（§5.4）
use axum::response::IntoResponse;
use axum::{
    middleware as axum_mw,
    routing::{get, post},
    Router,
};

use crate::assets;
use crate::error::AppError;
use crate::handlers::health;
use crate::handlers::{
    chapter_draft, characters, config as config_handler, export_import, items, novel_import,
    novels, outline, recycle, sandbox, search, sse,
};
use crate::middleware::{body_limit, host_guard, idempotency, request_log};
use crate::state::AppState;

/// D-28：仅开发模式放行 Vite dev server 跨源。
pub fn dev_cors_layer() -> tower_http::cors::CorsLayer {
    tower_http::cors::CorsLayer::new().allow_origin(tower_http::cors::AllowOrigin::exact(
        axum::http::HeaderValue::from_static("http://localhost:5173"),
    ))
}

pub fn build_router(state: AppState) -> Router {
    let body_limit = state.config.load().http.body_limit_bytes as usize;
    let import_limit = state.config.load().http.import_body_limit_bytes as usize;
    // /api/v1 服务层
    let v1 = Router::new()
        // novels
        .route(
            "/novels",
            get(novels::list_novels).post(novels::create_novel),
        )
        .route(
            "/novels/:id",
            get(novels::get_novel)
                .patch(novels::patch_novel)
                .delete(novels::delete_novel),
        )
        // characters
        .route(
            "/novels/:nid/characters",
            get(characters::list_characters).post(characters::create_character),
        )
        .route(
            "/novels/:nid/characters/:cid",
            get(characters::get_character)
                .patch(characters::patch_character)
                .delete(characters::delete_character),
        )
        .route(
            "/novels/:nid/relations",
            get(characters::get_relations).put(characters::put_relations),
        )
        // volumes
        .route(
            "/novels/:nid/volumes",
            get(outline::list_volumes).post(outline::create_volume),
        )
        .route(
            "/novels/:nid/volumes/:vid",
            get(outline::get_volume)
                .patch(outline::patch_volume)
                .delete(outline::delete_volume),
        )
        .route(
            "/novels/:nid/volumes/:vid/move",
            axum::routing::patch(outline::move_volume),
        )
        .route(
            "/novels/:nid/volumes/:vid/chapters",
            get(outline::list_chapters).post(outline::create_chapter),
        )
        .route(
            "/novels/:nid/chapters/:cid",
            get(outline::get_chapter)
                .patch(outline::patch_chapter)
                .delete(outline::delete_chapter),
        )
        // items
        .route(
            "/novels/:nid/items",
            get(items::list_items).post(items::create_item),
        )
        .route(
            "/novels/:nid/items/:iid",
            get(items::get_item)
                .patch(items::patch_item)
                .delete(items::delete_item),
        )
        // recycle
        .route("/recycle-bin", get(recycle::list_recycle))
        .route("/recycle-bin/restore", post(recycle::restore_recycle))
        .route(
            "/recycle-bin/:etype/:id",
            axum::routing::delete(recycle::purge_recycle),
        )
        // search
        .route(
            "/chapters/:cid/sandbox-sessions",
            post(sandbox::create_chapter_session),
        )
        .route(
            "/novels/:nid/sandbox-sessions",
            get(sandbox::list_sessions).post(sandbox::create_exploration_session),
        )
        .route("/sandbox-sessions/:sid", get(sandbox::get_session))
        .route(
            "/sandbox-sessions/:sid/control",
            post(sandbox::control_session),
        )
        .route("/sandbox-sessions/:sid/input", post(sandbox::input_session))
        .route("/sandbox-sessions/:sid/events", get(sse::stream_events))
        .route("/novels/:id/export", get(export_import::export_novel))
        .route(
            "/config",
            get(config_handler::get_config).patch(config_handler::patch_config),
        )
        .route("/usage", get(config_handler::get_usage))
        .route("/client-errors", post(config_handler::client_error))
        .route("/search", get(search::search_handler))
        .fallback(api_not_found)
        .with_state(state.clone())
        .layer(axum_mw::from_fn_with_state(
            state.clone(),
            idempotency::idempotency,
        ))
        .layer(body_limit::global_layer(body_limit));

    let import_router = Router::new()
        .route("/v1/import", post(export_import::import_package))
        .with_state(state.clone())
        .layer(body_limit::import_layer(import_limit))
        .layer(axum_mw::from_fn_with_state(
            state.clone(),
            idempotency::idempotency,
        ));

    let novel_import_router = novel_import::routes()
        .with_state(state.clone())
        .layer(body_limit::import_layer(
            body_limit::NOVEL_IMPORT_BODY_LIMIT_BYTES,
        ))
        .layer(axum_mw::from_fn_with_state(
            state.clone(),
            idempotency::idempotency,
        ));

    let api = Router::new()
        .route("/healthz", get(health::healthz))
        .route("/health", get(health::health))
        .nest("/v1", v1)
        .merge(import_router)
        .merge(novel_import_router)
        .merge(chapter_draft::router())
        .fallback(api_not_found)
        .with_state(state.clone());

    let mut router = Router::new()
        .nest("/api", api)
        .fallback(assets::serve)
        // 执行顺序（外→内）：request_log → host_guard → 路由级 body limit → handler
        .layer(axum_mw::from_fn_with_state(
            state.clone(),
            host_guard::host_guard,
        ))
        .layer(axum_mw::from_fn(request_log::request_log));

    if state.dev_cors {
        router = router.layer(dev_cors_layer());
    }

    router.with_state(state)
}

async fn api_not_found() -> axum::response::Response {
    AppError::route_not_found().into_response()
}
