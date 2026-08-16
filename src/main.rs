use std::sync::Arc;

use clap::Parser;

use vm_prom::config::Config;
use vm_prom::eventlog::EventlogTracker;
use vm_prom::hub::HubClient;
use vm_prom::routes::{self, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::parse();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(config.log_level.clone()))
        .init();

    let hub = Arc::new(HubClient::new(
        config.hub_url.clone(),
        config.scrape_timeout,
        config.insecure_skip_verify,
    ));

    let state = AppState {
        hub,
        metrics_path: config.metrics_path.clone(),
        eventlog_tracker: Arc::new(EventlogTracker::new()),
        eventlog_log_level: config.eventlog_log_level,
        scrape_fetches_eventlog: config.scrape_fetches_eventlog,
    };

    let app = routes::router(state);

    tracing::info!(
        address = %config.listen_address,
        metrics_path = %config.metrics_path,
        hub_url = %config.hub_url,
        scrape_fetches_eventlog = %config.scrape_fetches_eventlog,
        eventlog_log_level = %config.eventlog_log_level,
        "starting vm-prom"
    );

    let listener = tokio::net::TcpListener::bind(config.listen_address).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
