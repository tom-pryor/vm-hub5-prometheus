use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use vm_prom::hub::HubClient;
use vm_prom::routes::{AppState, router};

const DOWNSTREAM_SAMPLE: &str = include_str!("fixtures/downstream.json");
const UPSTREAM_SAMPLE: &str = include_str!("fixtures/upstream.json");

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

    let hub = Arc::new(HubClient::new(mock_server.uri(), Duration::from_secs(5), true));
    let state = AppState {
        hub,
        metrics_path: "/metrics".to_string(),
    };
    let app = router(state);

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
    let state = AppState {
        hub,
        metrics_path: "/metrics".to_string(),
    };
    let app = router(state);

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
