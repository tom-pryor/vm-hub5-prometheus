use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct DownstreamResponse {
    pub downstream: DownstreamChannels,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DownstreamChannels {
    pub channels: Vec<DownstreamChannel>,
}

/// A downstream channel, discriminated by `channelType`. Firmware may report
/// channel types we don't know about yet (`Unknown`) — those are skipped
/// when mapping to metrics rather than failing the whole scrape.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "channelType", rename_all = "snake_case")]
pub enum DownstreamChannel {
    ScQam(ScQamDownstreamChannel),
    Ofdm(OfdmDownstreamChannel),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScQamDownstreamChannel {
    #[serde(rename = "channelId")]
    pub channel_id: u32,
    pub frequency: f64,
    pub power: f64,
    pub modulation: String,
    pub snr: f64,
    #[serde(rename = "rxMer")]
    pub rx_mer: f64,
    #[serde(rename = "correctedErrors")]
    pub corrected_errors: u64,
    #[serde(rename = "uncorrectedErrors")]
    pub uncorrected_errors: u64,
    #[serde(rename = "lockStatus")]
    pub lock_status: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OfdmDownstreamChannel {
    #[serde(rename = "channelId")]
    pub channel_id: u32,
    #[serde(rename = "channelWidth")]
    pub channel_width: f64,
    pub modulation: String,
    #[serde(rename = "rxMer")]
    pub rx_mer: f64,
    pub power: f64,
    #[serde(rename = "correctedErrors")]
    pub corrected_errors: u64,
    #[serde(rename = "uncorrectedErrors")]
    pub uncorrected_errors: u64,
    #[serde(rename = "lockStatus")]
    pub lock_status: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamResponse {
    pub upstream: UpstreamChannels,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamChannels {
    pub channels: Vec<UpstreamChannel>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "channelType", rename_all = "snake_case")]
pub enum UpstreamChannel {
    Atdma(AtdmaUpstreamChannel),
    Ofdma(OfdmaUpstreamChannel),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AtdmaUpstreamChannel {
    #[serde(rename = "channelId")]
    pub channel_id: u32,
    pub frequency: f64,
    pub power: f64,
    #[serde(rename = "symbolRate")]
    pub symbol_rate: f64,
    pub modulation: String,
    #[serde(rename = "t1Timeout")]
    pub t1_timeout: u64,
    #[serde(rename = "t2Timeout")]
    pub t2_timeout: u64,
    #[serde(rename = "t3Timeout")]
    pub t3_timeout: u64,
    #[serde(rename = "t4Timeout")]
    pub t4_timeout: u64,
    #[serde(rename = "lockStatus")]
    pub lock_status: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OfdmaUpstreamChannel {
    #[serde(rename = "channelId")]
    pub channel_id: u32,
    #[serde(rename = "channelWidth")]
    pub channel_width: f64,
    pub power: f64,
    pub modulation: String,
    #[serde(rename = "t3Timeout")]
    pub t3_timeout: u64,
    #[serde(rename = "t4Timeout")]
    pub t4_timeout: u64,
    #[serde(rename = "lockStatus")]
    pub lock_status: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real sample payloads captured from a Virgin Media Hub 5.
    const DOWNSTREAM_SAMPLE: &str = include_str!("../../tests/fixtures/downstream.json");
    const UPSTREAM_SAMPLE: &str = include_str!("../../tests/fixtures/upstream.json");

    #[test]
    fn parses_downstream_sample() {
        let resp: DownstreamResponse = serde_json::from_str(DOWNSTREAM_SAMPLE).unwrap();
        assert_eq!(resp.downstream.channels.len(), 33);

        let sc_qam_count = resp
            .downstream
            .channels
            .iter()
            .filter(|c| matches!(c, DownstreamChannel::ScQam(_)))
            .count();
        assert_eq!(sc_qam_count, 32);

        let ofdm = resp
            .downstream
            .channels
            .iter()
            .find_map(|c| match c {
                DownstreamChannel::Ofdm(o) => Some(o),
                _ => None,
            })
            .expect("expected one ofdm channel");
        assert_eq!(ofdm.channel_id, 159);
        assert_eq!(ofdm.rx_mer, 350.0);
        assert_eq!(ofdm.power, -93.0);
        assert!(ofdm.lock_status);

        let first = match &resp.downstream.channels[0] {
            DownstreamChannel::ScQam(c) => c,
            other => panic!("expected sc_qam, got {other:?}"),
        };
        assert_eq!(first.channel_id, 1);
        assert_eq!(first.frequency, 139_000_000.0);
        assert_eq!(first.power, 6.1);
        assert_eq!(first.modulation, "qam_256");
        assert_eq!(first.snr, 41.0);
        assert_eq!(first.corrected_errors, 31);
        assert_eq!(first.uncorrected_errors, 0);
        assert!(first.lock_status);
    }

    #[test]
    fn parses_upstream_sample() {
        let resp: UpstreamResponse = serde_json::from_str(UPSTREAM_SAMPLE).unwrap();
        assert_eq!(resp.upstream.channels.len(), 6);

        let atdma_count = resp
            .upstream
            .channels
            .iter()
            .filter(|c| matches!(c, UpstreamChannel::Atdma(_)))
            .count();
        assert_eq!(atdma_count, 5);

        let ofdma = resp
            .upstream
            .channels
            .iter()
            .find_map(|c| match c {
                UpstreamChannel::Ofdma(o) => Some(o),
                _ => None,
            })
            .expect("expected one ofdma channel");
        assert_eq!(ofdma.channel_id, 6);
        assert_eq!(ofdma.power, 367.0);
        assert_eq!(ofdma.t3_timeout, 0);
        assert_eq!(ofdma.t4_timeout, 0);
        assert!(ofdma.lock_status);

        let first = match &resp.upstream.channels[0] {
            UpstreamChannel::Atdma(c) => c,
            other => panic!("expected atdma, got {other:?}"),
        };
        assert_eq!(first.channel_id, 1);
        assert_eq!(first.frequency, 49_600_000.0);
        assert_eq!(first.power, 42.8);
        assert_eq!(first.symbol_rate, 5120.0);
        assert_eq!(first.modulation, "qam_64");
        assert!(first.lock_status);
    }

    #[test]
    fn unrecognized_channel_type_is_skipped_not_fatal() {
        let json = r#"{"downstream":{"channels":[{"channelType":"some_future_type","channelId":99}]}}"#;
        let resp: DownstreamResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(
            resp.downstream.channels[0],
            DownstreamChannel::Unknown
        ));
    }
}
