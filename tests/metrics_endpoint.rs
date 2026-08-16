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

const DOWNSTREAM_SAMPLE: &str = include_str!("fixtures/downstream.json");
const UPSTREAM_SAMPLE: &str = include_str!("fixtures/upstream.json");
const EVENTLOG_SAMPLE: &str = include_str!("fixtures/eventlog.json");

fn test_state(hub: Arc<HubClient>, scrape_fetches_eventlog: bool) -> AppState {
    AppState {
        hub,
        metrics_path: "/metrics".to_string(),
        eventlog_tracker: Arc::new(EventlogTracker::new()),
        eventlog_log_level: tracing::Level::INFO,
        scrape_fetches_eventlog,
    }
}

/// Polls `mock_server.received_requests()` until at least one request to
/// `expected_path` shows up, or a short timeout elapses. Needed because the
/// `/metrics`-triggered eventlog fetch runs in a detached `tokio::spawn` and
/// isn't awaited by the handler, so it may not have reached the mock server
/// yet by the time the `/metrics` response comes back.
async fn wait_for_request(mock_server: &MockServer, expected_path: &str) -> bool {
    for _ in 0..100 {
        let requests = mock_server.received_requests().await.unwrap();
        if requests.iter().any(|r| r.url.path() == expected_path) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

#[tokio::test]
async fn metrics_endpoint_reports_real_sample_data() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/downstream"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(DOWNSTREAM_SAMPLE, "application/json"))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/upstream"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(UPSTREAM_SAMPLE, "application/json"))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/eventlog"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(EVENTLOG_SAMPLE, "application/json"))
        .mount(&mock_server)
        .await;

    let hub = Arc::new(HubClient::new(mock_server.uri(), Duration::from_secs(5), true));
    let app = router(test_state(hub, false));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();

    assert!(text.contains("docsis_scrape_success 1"));
    assert!(text.contains(
        r#"docsis_downstream_power_dbmv{channel_id="1",channel_type="sc_qam",modulation="qam_256"} 6.1"#
    ));
    assert!(text.contains(
        r#"docsis_downstream_power_dbmv{channel_id="159",channel_type="ofdm",modulation="qam_4096"} -9.3"#
    ));
    assert!(text.contains(
        r#"docsis_upstream_power_dbmv{channel_id="1",channel_type="atdma",modulation="qam_64"} 42.8"#
    ));
    assert!(text.contains(
        r#"docsis_upstream_power_dbmv{channel_id="6",channel_type="ofdma",modulation="qam_256"} 36.7"#
    ));
}

#[tokio::test]
async fn metrics_endpoint_reports_failure_when_hub_unreachable() {
    // Nothing is listening on this port, so both hub requests fail fast.
    let hub = Arc::new(HubClient::new(
        "http://127.0.0.1:1".to_string(),
        Duration::from_millis(500),
        true,
    ));
    let app = router(test_state(hub, false));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("docsis_scrape_success 0"));
}

async fn mount_downstream_upstream_mocks(mock_server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/downstream"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(DOWNSTREAM_SAMPLE, "application/json"))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/upstream"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(UPSTREAM_SAMPLE, "application/json"))
        .mount(mock_server)
        .await;
}

#[tokio::test]
async fn metrics_scrape_triggers_eventlog_fetch_when_enabled() {
    let mock_server = MockServer::start().await;
    mount_downstream_upstream_mocks(&mock_server).await;
    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/eventlog"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(EVENTLOG_SAMPLE, "application/json"))
        .mount(&mock_server)
        .await;

    let hub = Arc::new(HubClient::new(mock_server.uri(), Duration::from_secs(5), true));
    let app = router(test_state(hub, true));

    app.oneshot(
        Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();

    assert!(
        wait_for_request(&mock_server, "/rest/v1/cablemodem/eventlog").await,
        "expected /metrics scrape to trigger an eventlog fetch"
    );
}

#[tokio::test]
async fn metrics_scrape_skips_eventlog_fetch_when_disabled() {
    let mock_server = MockServer::start().await;
    mount_downstream_upstream_mocks(&mock_server).await;
    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/eventlog"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(EVENTLOG_SAMPLE, "application/json"))
        .mount(&mock_server)
        .await;

    let hub = Arc::new(HubClient::new(mock_server.uri(), Duration::from_secs(5), true));
    let app = router(test_state(hub, false));

    app.oneshot(
        Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();

    // Give a would-be spawned fetch a moment to appear, then confirm it never did.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let requests = mock_server.received_requests().await.unwrap();
    assert!(
        !requests
            .iter()
            .any(|r| r.url.path() == "/rest/v1/cablemodem/eventlog"),
        "expected no eventlog fetch when SCRAPE_FETCHES_EVENTLOG is disabled"
    );
}

#[tokio::test]
async fn metrics_endpoint_ignores_eventlog_fetch_failure() {
    let mock_server = MockServer::start().await;
    mount_downstream_upstream_mocks(&mock_server).await;
    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/eventlog"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let hub = Arc::new(HubClient::new(mock_server.uri(), Duration::from_secs(5), true));
    let app = router(test_state(hub, true));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(text.contains("docsis_scrape_success 1"));
}

#[tokio::test]
async fn metrics_scrape_does_not_duplicate_in_flight_eventlog_fetch() {
    let mock_server = MockServer::start().await;
    mount_downstream_upstream_mocks(&mock_server).await;
    Mock::given(method("GET"))
        .and(path("/rest/v1/cablemodem/eventlog"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(EVENTLOG_SAMPLE, "application/json")
                .set_delay(Duration::from_millis(300)),
        )
        .mount(&mock_server)
        .await;

    let hub = Arc::new(HubClient::new(mock_server.uri(), Duration::from_secs(5), true));
    let state = test_state(hub, true);
    let app = router(state);

    // Fire two scrapes back to back; the first eventlog fetch will still be
    // in flight (delayed 300ms) when the second scrape spawns its own.
    let app2 = app.clone();
    let first = app.oneshot(
        Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap(),
    );
    let second = app2.oneshot(
        Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap(),
    );
    let _ = tokio::join!(first, second);

    // Wait past the mocked delay so any spawned fetch has had time to land.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let requests = mock_server.received_requests().await.unwrap();
    let eventlog_requests = requests
        .iter()
        .filter(|r| r.url.path() == "/rest/v1/cablemodem/eventlog")
        .count();
    assert_eq!(
        eventlog_requests, 1,
        "expected only one eventlog fetch while the first was still in flight"
    );
}
