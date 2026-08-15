use std::net::SocketAddr;
use std::time::Duration;

use clap::Parser;

/// DOCSIS Prometheus exporter for the Virgin Media Hub 5.
#[derive(Parser, Debug, Clone)]
#[command(name = "vm-prom", version, about)]
pub struct Config {
    /// Base URL of the cable modem's REST API.
    #[arg(long, env = "HUB_URL", default_value = "https://192.168.100.1")]
    pub hub_url: String,

    /// Address the exporter's HTTP server listens on.
    #[arg(long, env = "LISTEN_ADDRESS", default_value = "0.0.0.0:9938")]
    pub listen_address: SocketAddr,

    /// Path the Prometheus metrics are served on.
    #[arg(long, env = "METRICS_PATH", default_value = "/metrics")]
    pub metrics_path: String,

    /// Timeout for each request made to the hub while scraping.
    #[arg(long, env = "SCRAPE_TIMEOUT", default_value = "5s", value_parser = parse_duration)]
    pub scrape_timeout: Duration,

    /// Skip TLS certificate verification when talking to the hub (it presents
    /// a self-signed certificate on its LAN-only management interface).
    #[arg(long, env = "INSECURE_SKIP_VERIFY", default_value = "true")]
    pub insecure_skip_verify: bool,

    /// Log level (error, warn, info, debug, trace).
    #[arg(long, env = "LOG_LEVEL", default_value = "info")]
    pub log_level: String,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num, suffix) = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .map(|i| s.split_at(i))
        .unwrap_or((s, "s"));

    let value: f64 = num
        .parse()
        .map_err(|_| format!("invalid duration value: {s}"))?;

    let multiplier = match suffix {
        "" | "s" => 1.0,
        "ms" => 0.001,
        "m" => 60.0,
        "h" => 3600.0,
        other => return Err(format!("unsupported duration suffix: {other}")),
    };

    Ok(Duration::from_secs_f64(value * multiplier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_seconds() {
        assert_eq!(parse_duration("5").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_duration("5s").unwrap(), Duration::from_secs(5));
    }

    #[test]
    fn parses_other_units() {
        assert_eq!(parse_duration("250ms").unwrap(), Duration::from_millis(250));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
    }

    #[test]
    fn rejects_unknown_suffix() {
        assert!(parse_duration("5x").is_err());
    }
}
