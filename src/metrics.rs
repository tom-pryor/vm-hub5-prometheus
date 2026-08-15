use std::time::Duration;

use prometheus::{Encoder, Gauge, GaugeVec, IntCounterVec, Opts, Registry, TextEncoder};

use crate::hub::models::{DownstreamChannel, DownstreamResponse, UpstreamChannel, UpstreamResponse};

const LABELS: &[&str] = &["channel_id", "channel_type", "modulation"];

/// OFDM/OFDMA channels report `power` and `rxMer` as integers roughly 10x the
/// magnitude of the equivalent SC-QAM/ATDMA float values (e.g. an OFDM power
/// reading of `-93` alongside SC-QAM readings like `6.1`). This divides by 10
/// to normalize them into the same dBmV/dB units. This is an inference from
/// sample data, not confirmed against Hub 5 documentation — verify against
/// the hub's own diagnostics page after deployment.
fn scale_ofdm(raw: f64) -> f64 {
    raw / 10.0
}

fn lock_value(locked: bool) -> f64 {
    if locked { 1.0 } else { 0.0 }
}

/// Renders the current scrape as Prometheus text exposition format.
///
/// `downstream`/`upstream` are `None` when the corresponding hub request
/// failed; in that case only the health metrics are emitted for that side.
pub fn render(
    downstream: Option<&DownstreamResponse>,
    upstream: Option<&UpstreamResponse>,
    success: bool,
    duration: Duration,
) -> String {
    let registry = Registry::new();

    register_downstream(&registry, downstream);
    register_upstream(&registry, upstream);
    register_health(&registry, success, duration);

    let metric_families = registry.gather();
    let mut buf = Vec::new();
    TextEncoder::new()
        .encode(&metric_families, &mut buf)
        .expect("encoding prometheus metrics");
    String::from_utf8(buf).expect("prometheus text encoder produces valid utf8")
}

fn register_downstream(registry: &Registry, downstream: Option<&DownstreamResponse>) {
    let power = gauge_vec(
        "docsis_downstream_power_dbmv",
        "Downstream power level in dBmV",
    );
    let snr = gauge_vec("docsis_downstream_snr_db", "Downstream SNR in dB (sc_qam only)");
    let rx_mer = gauge_vec("docsis_downstream_rxmer_db", "Downstream RxMER in dB");
    let frequency = gauge_vec(
        "docsis_downstream_frequency_hz",
        "Downstream channel center frequency in Hz (sc_qam only)",
    );
    let channel_width = gauge_vec(
        "docsis_downstream_channel_width_hz",
        "Downstream channel width in Hz (ofdm only)",
    );
    let corrected = counter_vec(
        "docsis_downstream_corrected_errors_total",
        "Cumulative corrected codeword errors reported by the modem",
    );
    let uncorrected = counter_vec(
        "docsis_downstream_uncorrected_errors_total",
        "Cumulative uncorrected codeword errors reported by the modem",
    );
    let lock = gauge_vec(
        "docsis_downstream_lock_status",
        "Whether the downstream channel is locked (1) or not (0)",
    );

    for collector in [&power, &snr, &rx_mer, &frequency, &channel_width, &lock] {
        registry
            .register(Box::new(collector.clone()))
            .expect("registering downstream gauge");
    }
    for collector in [&corrected, &uncorrected] {
        registry
            .register(Box::new(collector.clone()))
            .expect("registering downstream counter");
    }

    let Some(downstream) = downstream else {
        return;
    };

    for channel in &downstream.downstream.channels {
        match channel {
            DownstreamChannel::ScQam(c) => {
                let id = c.channel_id.to_string();
                let labels = [id.as_str(), "sc_qam", c.modulation.as_str()];
                power.with_label_values(&labels).set(c.power);
                snr.with_label_values(&labels).set(c.snr);
                rx_mer.with_label_values(&labels).set(c.rx_mer);
                frequency.with_label_values(&labels).set(c.frequency);
                corrected
                    .with_label_values(&labels)
                    .inc_by(c.corrected_errors);
                uncorrected
                    .with_label_values(&labels)
                    .inc_by(c.uncorrected_errors);
                lock.with_label_values(&labels).set(lock_value(c.lock_status));
            }
            DownstreamChannel::Ofdm(c) => {
                let id = c.channel_id.to_string();
                let labels = [id.as_str(), "ofdm", c.modulation.as_str()];
                power.with_label_values(&labels).set(scale_ofdm(c.power));
                rx_mer.with_label_values(&labels).set(scale_ofdm(c.rx_mer));
                channel_width.with_label_values(&labels).set(c.channel_width);
                corrected
                    .with_label_values(&labels)
                    .inc_by(c.corrected_errors);
                uncorrected
                    .with_label_values(&labels)
                    .inc_by(c.uncorrected_errors);
                lock.with_label_values(&labels).set(lock_value(c.lock_status));
            }
            DownstreamChannel::Unknown => {
                tracing::warn!("skipping downstream channel with unrecognized channelType");
            }
        }
    }
}

fn register_upstream(registry: &Registry, upstream: Option<&UpstreamResponse>) {
    let power = gauge_vec("docsis_upstream_power_dbmv", "Upstream power level in dBmV");
    let frequency = gauge_vec(
        "docsis_upstream_frequency_hz",
        "Upstream channel center frequency in Hz (atdma only)",
    );
    let symbol_rate = gauge_vec(
        "docsis_upstream_symbol_rate_ksps",
        "Upstream symbol rate in ksym/s (atdma only)",
    );
    let channel_width = gauge_vec(
        "docsis_upstream_channel_width_hz",
        "Upstream channel width in Hz (ofdma only)",
    );
    let t1 = counter_vec(
        "docsis_upstream_t1_timeouts_total",
        "Cumulative T1 timeouts reported by the modem (atdma only)",
    );
    let t2 = counter_vec(
        "docsis_upstream_t2_timeouts_total",
        "Cumulative T2 timeouts reported by the modem (atdma only)",
    );
    let t3 = counter_vec(
        "docsis_upstream_t3_timeouts_total",
        "Cumulative T3 timeouts reported by the modem",
    );
    let t4 = counter_vec(
        "docsis_upstream_t4_timeouts_total",
        "Cumulative T4 timeouts reported by the modem",
    );
    let lock = gauge_vec(
        "docsis_upstream_lock_status",
        "Whether the upstream channel is locked (1) or not (0)",
    );

    for collector in [&power, &frequency, &symbol_rate, &channel_width, &lock] {
        registry
            .register(Box::new(collector.clone()))
            .expect("registering upstream gauge");
    }
    for collector in [&t1, &t2, &t3, &t4] {
        registry
            .register(Box::new(collector.clone()))
            .expect("registering upstream counter");
    }

    let Some(upstream) = upstream else {
        return;
    };

    for channel in &upstream.upstream.channels {
        match channel {
            UpstreamChannel::Atdma(c) => {
                let id = c.channel_id.to_string();
                let labels = [id.as_str(), "atdma", c.modulation.as_str()];
                power.with_label_values(&labels).set(c.power);
                frequency.with_label_values(&labels).set(c.frequency);
                symbol_rate.with_label_values(&labels).set(c.symbol_rate);
                t1.with_label_values(&labels).inc_by(c.t1_timeout);
                t2.with_label_values(&labels).inc_by(c.t2_timeout);
                t3.with_label_values(&labels).inc_by(c.t3_timeout);
                t4.with_label_values(&labels).inc_by(c.t4_timeout);
                lock.with_label_values(&labels).set(lock_value(c.lock_status));
            }
            UpstreamChannel::Ofdma(c) => {
                let id = c.channel_id.to_string();
                let labels = [id.as_str(), "ofdma", c.modulation.as_str()];
                power.with_label_values(&labels).set(scale_ofdm(c.power));
                channel_width.with_label_values(&labels).set(c.channel_width);
                t3.with_label_values(&labels).inc_by(c.t3_timeout);
                t4.with_label_values(&labels).inc_by(c.t4_timeout);
                lock.with_label_values(&labels).set(lock_value(c.lock_status));
            }
            UpstreamChannel::Unknown => {
                tracing::warn!("skipping upstream channel with unrecognized channelType");
            }
        }
    }
}

fn register_health(registry: &Registry, success: bool, duration: Duration) {
    let scrape_success = Gauge::with_opts(Opts::new(
        "docsis_scrape_success",
        "Whether the last scrape of the hub's cablemodem endpoints fully succeeded (1) or not (0)",
    ))
    .expect("building docsis_scrape_success gauge");
    scrape_success.set(if success { 1.0 } else { 0.0 });
    registry
        .register(Box::new(scrape_success))
        .expect("registering docsis_scrape_success");

    let scrape_duration = Gauge::with_opts(Opts::new(
        "docsis_scrape_duration_seconds",
        "Duration of the last scrape of the hub's cablemodem endpoints in seconds",
    ))
    .expect("building docsis_scrape_duration_seconds gauge");
    scrape_duration.set(duration.as_secs_f64());
    registry
        .register(Box::new(scrape_duration))
        .expect("registering docsis_scrape_duration_seconds");
}

fn gauge_vec(name: &str, help: &str) -> GaugeVec {
    GaugeVec::new(Opts::new(name, help), LABELS).expect("building gauge vec")
}

fn counter_vec(name: &str, help: &str) -> IntCounterVec {
    IntCounterVec::new(Opts::new(name, help), LABELS).expect("building counter vec")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOWNSTREAM_SAMPLE: &str = include_str!("../tests/fixtures/downstream.json");
    const UPSTREAM_SAMPLE: &str = include_str!("../tests/fixtures/upstream.json");

    fn sample_downstream() -> DownstreamResponse {
        serde_json::from_str(DOWNSTREAM_SAMPLE).unwrap()
    }

    fn sample_upstream() -> UpstreamResponse {
        serde_json::from_str(UPSTREAM_SAMPLE).unwrap()
    }

    #[test]
    fn sc_qam_values_pass_through_unscaled() {
        let ds = sample_downstream();
        let out = render(Some(&ds), None, true, Duration::from_millis(1));
        assert!(out.contains(
            r#"docsis_downstream_power_dbmv{channel_id="1",channel_type="sc_qam",modulation="qam_256"} 6.1"#
        ));
        assert!(out.contains(
            r#"docsis_downstream_snr_db{channel_id="1",channel_type="sc_qam",modulation="qam_256"} 41"#
        ));
    }

    #[test]
    fn ofdm_power_and_rxmer_are_scaled_down_by_ten() {
        let ds = sample_downstream();
        let out = render(Some(&ds), None, true, Duration::from_millis(1));
        assert!(out.contains(
            r#"docsis_downstream_power_dbmv{channel_id="159",channel_type="ofdm",modulation="qam_4096"} -9.3"#
        ));
        assert!(out.contains(
            r#"docsis_downstream_rxmer_db{channel_id="159",channel_type="ofdm",modulation="qam_4096"} 35"#
        ));
    }

    #[test]
    fn ofdma_power_is_scaled_down_by_ten() {
        let us = sample_upstream();
        let out = render(None, Some(&us), true, Duration::from_millis(1));
        assert!(out.contains(
            r#"docsis_upstream_power_dbmv{channel_id="6",channel_type="ofdma",modulation="qam_256"} 36.7"#
        ));
    }

    #[test]
    fn atdma_power_passes_through_unscaled() {
        let us = sample_upstream();
        let out = render(None, Some(&us), true, Duration::from_millis(1));
        assert!(out.contains(
            r#"docsis_upstream_power_dbmv{channel_id="1",channel_type="atdma",modulation="qam_64"} 42.8"#
        ));
    }

    #[test]
    fn health_metrics_reflect_failure() {
        let out = render(None, None, false, Duration::from_secs(2));
        assert!(out.contains("docsis_scrape_success 0"));
        assert!(out.contains("docsis_scrape_duration_seconds 2"));
    }

    #[test]
    fn health_metrics_reflect_success() {
        let ds = sample_downstream();
        let us = sample_upstream();
        let out = render(Some(&ds), Some(&us), true, Duration::from_millis(1));
        assert!(out.contains("docsis_scrape_success 1"));
    }
}
