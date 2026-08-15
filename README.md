# vm-prom

A Prometheus exporter for DOCSIS cable modem stats from a **Virgin Media Hub 5**,
written in Rust. On every scrape of `/metrics` it fetches the hub's downstream and
upstream channel stats and re-exposes them in Prometheus text exposition format.

## How it works

The Hub 5 exposes its cable modem stats over its LAN-only management interface at
`https://192.168.100.1` (self-signed certificate, no authentication required):

- `GET /rest/v1/cablemodem/downstream`
- `GET /rest/v1/cablemodem/upstream`

`vm-prom` fetches both on every scrape (no background polling or caching), maps the
channels into Prometheus metrics, and returns them. If a hub request fails or times
out, `/metrics` still returns `200 OK` with `docsis_scrape_success 0` so Prometheus
sees a clean "exporter up, target down" signal rather than a failed scrape.

### A note on OFDM/OFDMA units

The Hub 5's `sc_qam` (downstream) and `atdma` (upstream) channels report `power` and
SNR/MER already scaled as floats (e.g. `6.1` dBmV). The `ofdm`/`ofdma` channels used
for DOCSIS 3.1 report those same fields as larger raw integers (e.g. `-93`, `350`,
`367`) that look like they're in tenths of a unit. `vm-prom` divides those by 10 to
normalize them into the same dBmV/dB scale as the other channel types. This is an
inference from sample data, not confirmed documentation — after deploying, compare
`docsis_downstream_power_dbmv`/`docsis_downstream_rxmer_db` for your `ofdm` channel
and `docsis_upstream_power_dbmv` for your `ofdma` channel against the values shown on
the Hub 5's own diagnostics page, and adjust `scale_ofdm()` in `src/metrics.rs` if
your hub disagrees.

## Metrics

All metrics are namespaced `docsis_` and labeled with `channel_id`, `channel_type`
(`sc_qam`/`ofdm` for downstream, `atdma`/`ofdma` for upstream), and `modulation`.
Fields a channel type doesn't report (e.g. `frequency` for OFDM channels) simply have
no series for that channel.

Downstream:

| Metric | Type | Notes |
|---|---|---|
| `docsis_downstream_power_dbmv` | gauge | |
| `docsis_downstream_snr_db` | gauge | `sc_qam` only |
| `docsis_downstream_rxmer_db` | gauge | |
| `docsis_downstream_frequency_hz` | gauge | `sc_qam` only |
| `docsis_downstream_channel_width_hz` | gauge | `ofdm` only |
| `docsis_downstream_corrected_errors_total` | counter | cumulative, as reported by the modem |
| `docsis_downstream_uncorrected_errors_total` | counter | cumulative |
| `docsis_downstream_lock_status` | gauge | 1 = locked |

Upstream:

| Metric | Type | Notes |
|---|---|---|
| `docsis_upstream_power_dbmv` | gauge | |
| `docsis_upstream_frequency_hz` | gauge | `atdma` only |
| `docsis_upstream_symbol_rate_ksps` | gauge | `atdma` only |
| `docsis_upstream_channel_width_hz` | gauge | `ofdma` only |
| `docsis_upstream_t1_timeouts_total`..`t4_timeouts_total` | counter | cumulative; `t1`/`t2` are `atdma` only |
| `docsis_upstream_lock_status` | gauge | 1 = locked |

Exporter health (unlabeled):

| Metric | Type |
|---|---|
| `docsis_scrape_success` | gauge (1/0) |
| `docsis_scrape_duration_seconds` | gauge |

## Configuration

All settings are environment variables, overridable by an equivalent CLI flag.

| Env var | Flag | Default | Description |
|---|---|---|---|
| `HUB_URL` | `--hub-url` | `https://192.168.100.1` | Base URL of the hub's cable modem API |
| `LISTEN_ADDRESS` | `--listen-address` | `0.0.0.0:9938` | Address the exporter's HTTP server listens on |
| `METRICS_PATH` | `--metrics-path` | `/metrics` | Path metrics are served on |
| `SCRAPE_TIMEOUT` | `--scrape-timeout` | `5s` | Per-request timeout when calling the hub (accepts `ms`/`s`/`m`/`h` suffixes) |
| `INSECURE_SKIP_VERIFY` | `--insecure-skip-verify` | `true` | Skip TLS certificate verification (the hub uses a self-signed cert) |
| `LOG_LEVEL` | `--log-level` | `info` | `tracing` log level |

## Running

### From source

```sh
cargo build --release
HUB_URL=https://192.168.100.1 ./target/release/vm-prom
curl http://localhost:9938/metrics
```

### Docker

```sh
docker build -t vm-prom .
docker run --rm -p 9938:9938 -e HUB_URL=https://192.168.100.1 vm-prom
```

Since the Hub 5's management IP (`192.168.100.1`) is only reachable when connected
directly to the hub (e.g. running on a machine plugged into it, or with `--network
host`), run the container with `--network host` if it can't otherwise reach that
address.

### systemd

```ini
[Unit]
Description=vm-prom DOCSIS exporter
After=network.target

[Service]
ExecStart=/usr/local/bin/vm-prom
Environment=HUB_URL=https://192.168.100.1
Environment=LISTEN_ADDRESS=0.0.0.0:9938
Restart=on-failure
DynamicUser=yes

[Install]
WantedBy=multi-user.target
```

### Prometheus scrape config

```yaml
scrape_configs:
  - job_name: vm-prom
    static_configs:
      - targets: ["localhost:9938"]
```

## Development

```sh
cargo build --release   # build
cargo test               # unit + integration tests (no real hardware needed —
                          # integration tests run against a mocked hub via wiremock)
cargo clippy              # lint
```

Test fixtures in `tests/fixtures/` are real sample payloads captured from a Hub 5.
