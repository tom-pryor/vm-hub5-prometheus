use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;

use crate::eventlog::EventlogTracker;
use crate::hub::HubClient;
use crate::hub::models::EventlogResponse;
use crate::metrics;

#[derive(Clone)]
pub struct AppState {
    pub hub: Arc<HubClient>,
    pub metrics_path: String,
    pub eventlog_tracker: Arc<EventlogTracker>,
    pub eventlog_log_level: tracing::Level,
    pub scrape_fetches_eventlog: bool,
}

/// Builds the exporter's router: `/` (index), the configured metrics path,
/// and the fixed `/events` path (hub eventlog passthrough).
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route(&state.metrics_path.clone(), get(metrics_handler))
        .route("/events", get(events_handler))
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

/// Fetches the hub's eventlog and returns it unchanged as JSON. Diverges
/// from `/metrics`'s "always 200" convention on purpose: `/events` isn't a
/// Prometheus scrape target, so a hub fetch failure is surfaced as a real
/// HTTP error (502) rather than folded into a body field.
pub async fn events_handler(
    State(state): State<AppState>,
) -> Result<Json<EventlogResponse>, StatusCode> {
    state
        .eventlog_tracker
        .fetch_and_log(&state.hub, state.eventlog_log_level)
        .await
        .map(Json)
        .map_err(|err| {
            tracing::warn!(error = %err, "failed to fetch eventlog from hub");
            StatusCode::BAD_GATEWAY
        })
}

pub async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    if state.scrape_fetches_eventlog {
        let hub = Arc::clone(&state.hub);
        let tracker = Arc::clone(&state.eventlog_tracker);
        let level = state.eventlog_log_level;
        tokio::spawn(async move {
            match tracker.fetch_and_log_if_idle(&hub, level).await {
                Some(Err(err)) => {
                    tracing::warn!(error = %err, "failed to fetch eventlog from hub during metrics scrape")
                }
                Some(Ok(_)) => {}
                None => tracing::debug!(
                    "skipped eventlog fetch during metrics scrape: previous fetch still in flight"
                ),
            }
        });
    }

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
