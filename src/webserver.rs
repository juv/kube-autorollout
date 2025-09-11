use axum::{http::StatusCode, response::IntoResponse, routing::get, Router};

pub async fn readiness_probe() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

pub async fn liveness_probe() -> impl IntoResponse {
    StatusCode::NO_CONTENT
}

pub fn create_app() -> Router {
    Router::new()
        .route("/health/live", get(liveness_probe))
        .route("/health/ready", get(readiness_probe))
}
