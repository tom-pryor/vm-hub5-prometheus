use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::extract::State;
use axum::http::header;
use axum::response::{Html, IntoResponse};
use axum::routing::get;

use crate::hub::HubClient;
use crate::metrics;

#[derive(Clone)]
pub struct AppState {
    pub hub: Arc<HubClient>,
    pub metrics_path: String,
}

/// Builds the exporter's router: `/` (index) and the configured metrics path.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route(&state.metrics_path.clone(), get(metrics_handler))
        .with_state(state)
}

pub async fn index(State(state): State<AppState>) -> Html<String> {
    Html(format!(
        "<html><head><title>vm-prom</title></head><body>\
         <h1>Virgin Media Hub 5 DOCSIS Exporter</h1>\
         <p><a href=\"{}\">Metrics</a></p>\
         </body></html>",
        state.metrics_path
    ))
}

pub async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let start = Instant::now();
    let (downstream, upstream) =
        tokio::join!(state.hub.fetch_downstream(), state.hub.fetch_upstream());
    let duration = start.elapsed();

    if let Err(err) = &downstream {
        tracing::warn!(error = %err, "failed to fetch downstream stats from hub");
    }
    if let Err(err) = &upstream {
        tracing::warn!(error = %err, "failed to fetch upstream stats from hub");
    }

    let success = downstream.is_ok() && upstream.is_ok();
    let body = metrics::render(
        downstream.as_ref().ok(),
        upstream.as_ref().ok(),
        success,
        duration,
    );

    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], body)
}
