# Network Monitor Agent

A Fing-like Rust agent for continuous LAN monitoring: device discovery, classification, notifications, and speed-test history.

## Features

- Continuous ARP scanning with optional passive capture (ARP/DHCP)
- Device join/leave/return detection with SQLite history
- Identity tagging (name household members across devices)
- Device type inference (router, mobile, desktop, OS hints)
- Multi-channel notifications: ntfy, webhooks, desktop (FCM stub for future Android app)
- Agent naming to disambiguate multiple monitors on one notification topic
- WiFi SSID in network up/down alerts (omitted when unknown)
- Periodic speed tests with time-series API for plotting
- REST API on `127.0.0.1:8080` by default

## System dependencies (Linux)

```bash
# Fedora
sudo dnf install libpcap-devel openssl-devel

# Debian/Ubuntu
sudo apt install libpcap-dev libssl-dev
```

## Configuration

| File | Purpose |
|---|---|
| `config/example.toml` | Setup template in the project tree — used only by `nm-agent setup` |
| `~/.config/network-monitor/config.toml` | Runtime config — read by `nm-agent run` |

Run `nm-agent setup` once to copy the template into your home directory and set the agent name. Edit `~/.config/network-monitor/config.toml` for ongoing changes.

## Quick start

```bash
# Build
cargo build --release

# First-run setup (template → ~/.config/network-monitor/config.toml)
cargo run -p nm-agent -- setup

# Run daemon (requires elevated privileges for ARP/pcap)
sudo -E cargo run -p nm-agent -- run

# One-shot scan
sudo -E cargo run -p nm-agent -- --scan-once
```

Override paths if needed:

```bash
cargo run -p nm-agent -- setup --template config/example.toml
cargo run -p nm-agent -- run --config ~/.config/network-monitor/config.toml
```

## Privileges

ARP scanning and passive capture require raw packet access:

```bash
sudo setcap cap_net_raw,cap_net_admin+ep target/release/nm-agent
```

Or run under `sudo` / use the provided systemd unit with `AmbientCapabilities`.

## Android notifications

Install the [ntfy app](https://ntfy.sh) and subscribe to your configured topic (e.g. `home-lan`). No custom Android app required for Phase 1.

## API endpoints

| Endpoint | Description |
|---|---|
| `GET /health` | Agent name, SSID, link state |
| `GET /devices` | All known devices |
| `PUT /devices/{mac}` | Tag/rename device |
| `GET/POST /identities` | Household identities |
| `GET /events` | Event log |
| `GET /speedtests` | Speed test time series |
| `POST /speedtests/run` | On-demand speed test |

See [`config/example.toml`](config/example.toml) for the setup template and `~/.config/network-monitor/config.toml` for your live settings.

## Workspace crates

- `nm-core` — types, config, notification templates
- `nm-store` — SQLite persistence and device registry
- `nm-discovery` — ARP, passive capture, mDNS, link monitor, SSID detection
- `nm-classify` — OUI and heuristic device classification
- `nm-notify` — ntfy, webhook, desktop, FCM stub
- `nm-speedtest` — periodic throughput tests
- `nm-api` — axum REST + SSE
- `nm-agent` — daemon binary

## systemd

```bash
sudo cp crates/nm-agent/nm-agent.service /etc/systemd/system/
sudo systemctl enable --now nm-agent
```
