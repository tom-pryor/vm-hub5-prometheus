use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use vm_prom::eventlog::EventlogTracker;
use vm_prom::hub::HubClient;
use vm_prom::routes::{AppState, router};

const EVENTLOG_SAMPLE: &str = include_str!("fixtures/eventlog.json");
const EVENTLOG_NEXT_SAMPLE: &str = include_str!("fixtures/eventlog_next.json");

fn test_state(hub: Arc<HubClient>) -> AppState {
    AppState {
        hub,
        metrics_path: "/metrics".to_string(),
        eventlog_tracker: Arc::new(EventlogTracker::new()),
        eventlog_log_level: tracing::Level::INFO,
        scrape_fetches_eventlog: true,
    }
}

#[tokio::test]
async fn events_endpoint_returns_hub_eventlog_json() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/eventlog"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(EVENTLOG_SAMPLE, "application/json"))
        .mount(&mock_server)
        .await;

    let hub = Arc::new(HubClient::new(mock_server.uri(), Duration::from_secs(5), true));
    let app = router(test_state(hub));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("CM-STATUS"));
    assert!(text.contains("2026-08-15T12:10:17.000Z"));
}

#[tokio::test]
async fn events_endpoint_returns_502_when_hub_unreachable() {
    let hub = Arc::new(HubClient::new(
        "http://127.0.0.1:1".to_string(),
        Duration::from_millis(500),
        true,
    ));
    let app = router(test_state(hub));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn repeated_events_fetch_only_logs_new_entries_on_the_second_call() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/eventlog"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(EVENTLOG_SAMPLE, "application/json"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/eventlog"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(EVENTLOG_NEXT_SAMPLE, "application/json"))
        .mount(&mock_server)
        .await;

    let hub = Arc::new(HubClient::new(mock_server.uri(), Duration::from_secs(5), true));
    let state = test_state(hub);
    let app = router(state);

    // First fetch: establishes the baseline (whole backlog is "new").
    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    // Second fetch: the hub now reports two additional, newer entries.
    let second = app
        .oneshot(
            Request::builder()
                .uri("/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    let body = to_bytes(second.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("2026-08-15T12:15:03.000Z"));
    assert!(text.contains("2026-08-15T12:12:41.000Z"));
}
