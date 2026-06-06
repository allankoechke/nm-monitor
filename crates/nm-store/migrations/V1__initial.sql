CREATE TABLE IF NOT EXISTS agent_settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS identities (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    notes TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS devices (
    mac TEXT PRIMARY KEY NOT NULL,
    current_ip TEXT,
    hostname TEXT,
    vendor TEXT,
    kind TEXT NOT NULL DEFAULT 'unknown',
    os_hint TEXT NOT NULL DEFAULT 'unknown',
    identity_id TEXT REFERENCES identities(id),
    user_label TEXT,
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    online INTEGER NOT NULL DEFAULT 0,
    open_ports TEXT NOT NULL DEFAULT '[]',
    mdns_services TEXT NOT NULL DEFAULT '[]',
    confidence REAL NOT NULL DEFAULT 0.0,
    inference_source TEXT,
    do_not_scan INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    network_name TEXT,
    device_mac TEXT,
    device_ip TEXT,
    message TEXT NOT NULL,
    details TEXT
);

CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);

CREATE TABLE IF NOT EXISTS speed_test_results (
    id TEXT PRIMARY KEY NOT NULL,
    timestamp TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    network_name TEXT,
    interface TEXT NOT NULL,
    download_mbps REAL,
    upload_mbps REAL,
    latency_ms REAL,
    jitter_ms REAL,
    packet_loss_pct REAL,
    server_name TEXT,
    test_duration_ms INTEGER,
    error TEXT
);

CREATE INDEX IF NOT EXISTS idx_speedtests_timestamp ON speed_test_results(timestamp);
