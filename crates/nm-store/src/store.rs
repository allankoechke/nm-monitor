use crate::error::StoreError;
use crate::migrations::run_migrations;
use chrono::{DateTime, Utc};
use mac_address::MacAddress;
use nm_core::{
    device::{Device, DeviceKind, OsHint},
    event::{EventKind, EventRecord},
    identity::Identity,
    speedtest::SpeedTestResult,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::str::FromStr;
use uuid::Uuid;

pub struct Store {
    conn: parking_lot::Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::InvalidData(e.to_string()))?;
        }
        let mut conn = Connection::open(path)?;
        run_migrations(&mut conn)?;
        Ok(Self {
            conn: parking_lot::Mutex::new(conn),
        })
    }

    pub fn get_agent_name(&self) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT value FROM agent_settings WHERE key = 'agent_name'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(StoreError::from)
    }

    pub fn set_agent_name(&self, name: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO agent_settings (key, value) VALUES ('agent_name', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![name],
        )?;
        Ok(())
    }

    pub fn upsert_device(&self, device: &Device) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO devices (
                mac, current_ip, hostname, vendor, kind, os_hint, identity_id, user_label,
                first_seen, last_seen, online, open_ports, mdns_services, confidence,
                inference_source, do_not_scan
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
            ON CONFLICT(mac) DO UPDATE SET
                current_ip=excluded.current_ip,
                hostname=COALESCE(excluded.hostname, devices.hostname),
                vendor=COALESCE(excluded.vendor, devices.vendor),
                kind=excluded.kind,
                os_hint=excluded.os_hint,
                identity_id=excluded.identity_id,
                user_label=excluded.user_label,
                last_seen=excluded.last_seen,
                online=excluded.online,
                open_ports=excluded.open_ports,
                mdns_services=excluded.mdns_services,
                confidence=excluded.confidence,
                inference_source=excluded.inference_source,
                do_not_scan=excluded.do_not_scan",
            params![
                device.mac.to_string(),
                device.current_ip.map(|ip| ip.to_string()),
                device.hostname,
                device.vendor,
                device.kind.to_string(),
                device.os_hint.to_string(),
                device.identity_id.map(|id| id.to_string()),
                device.user_label,
                device.first_seen.to_rfc3339(),
                device.last_seen.to_rfc3339(),
                device.online as i32,
                serde_json::to_string(&device.open_ports).unwrap_or_else(|_| "[]".into()),
                serde_json::to_string(&device.mdns_services).unwrap_or_else(|_| "[]".into()),
                device.confidence,
                device.inference_source,
                device.do_not_scan as i32,
            ],
        )?;
        Ok(())
    }

    pub fn get_device(&self, mac: &MacAddress) -> Result<Option<Device>, StoreError> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT mac, current_ip, hostname, vendor, kind, os_hint, identity_id, user_label,
                    first_seen, last_seen, online, open_ports, mdns_services, confidence,
                    inference_source, do_not_scan
             FROM devices WHERE mac = ?1",
            params![mac.to_string()],
            |row| parse_device_row(row),
        )
        .optional()
        .map_err(StoreError::from)
    }

    pub fn list_devices(&self) -> Result<Vec<Device>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT mac, current_ip, hostname, vendor, kind, os_hint, identity_id, user_label,
                    first_seen, last_seen, online, open_ports, mdns_services, confidence,
                    inference_source, do_not_scan
             FROM devices ORDER BY last_seen DESC",
        )?;
        let rows = stmt
            .query_map([], |row| parse_device_row(row))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn update_device_label(
        &self,
        mac: &MacAddress,
        user_label: Option<String>,
        identity_id: Option<Uuid>,
        kind: Option<DeviceKind>,
        os_hint: Option<OsHint>,
        do_not_scan: Option<bool>,
    ) -> Result<Device, StoreError> {
        let mut device = self
            .get_device(mac)?
            .ok_or_else(|| StoreError::NotFound(mac.to_string()))?;
        if let Some(label) = user_label {
            device.user_label = Some(label);
        }
        if let Some(id) = identity_id {
            device.identity_id = Some(id);
        }
        if let Some(k) = kind {
            device.kind = k;
        }
        if let Some(o) = os_hint {
            device.os_hint = o;
        }
        if let Some(s) = do_not_scan {
            device.do_not_scan = s;
        }
        self.upsert_device(&device)?;
        Ok(device)
    }

    pub fn create_identity(&self, display_name: &str, notes: Option<String>) -> Result<Identity, StoreError> {
        let identity = Identity {
            id: Uuid::new_v4(),
            display_name: display_name.to_string(),
            notes,
            created_at: Utc::now(),
        };
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO identities (id, display_name, notes, created_at) VALUES (?1,?2,?3,?4)",
            params![
                identity.id.to_string(),
                identity.display_name,
                identity.notes,
                identity.created_at.to_rfc3339(),
            ],
        )?;
        Ok(identity)
    }

    pub fn list_identities(&self) -> Result<Vec<Identity>, StoreError> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT id, display_name, notes, created_at FROM identities ORDER BY display_name")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Identity {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    display_name: row.get(1)?,
                    notes: row.get(2)?,
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_identity(&self, id: &Uuid) -> Result<Option<Identity>, StoreError> {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT id, display_name, notes, created_at FROM identities WHERE id = ?1",
            params![id.to_string()],
            |row| {
                Ok(Identity {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    display_name: row.get(1)?,
                    notes: row.get(2)?,
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            },
        )
        .optional()
        .map_err(StoreError::from)
    }

    pub fn insert_event(&self, event: &EventRecord) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO events (id, kind, timestamp, agent_name, network_name, device_mac,
                                 device_ip, message, details)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                event.id.to_string(),
                serde_json::to_string(&event.kind).unwrap_or_default().trim_matches('"'),
                event.timestamp.to_rfc3339(),
                event.agent_name,
                event.network_name,
                event.device_mac.map(|m| m.to_string()),
                event.device_ip.map(|ip| ip.to_string()),
                event.message,
                event
                    .details
                    .as_ref()
                    .map(|d| d.to_string()),
            ],
        )?;
        Ok(())
    }

    pub fn list_events(
        &self,
        limit: usize,
        kind: Option<EventKind>,
    ) -> Result<Vec<EventRecord>, StoreError> {
        let conn = self.conn.lock();
        let mut events = Vec::new();
        if let Some(k) = kind {
            let kind_str = serde_json::to_string(&k).unwrap_or_default().trim_matches('"').to_string();
            let mut stmt = conn.prepare(
                "SELECT id, kind, timestamp, agent_name, network_name, device_mac, device_ip, message, details
                 FROM events WHERE kind = ?1 ORDER BY timestamp DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![kind_str, limit as i64], parse_event_row)?;
            for row in rows {
                events.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, kind, timestamp, agent_name, network_name, device_mac, device_ip, message, details
                 FROM events ORDER BY timestamp DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit as i64], parse_event_row)?;
            for row in rows {
                events.push(row?);
            }
        }
        Ok(events)
    }

    pub fn insert_speed_test(&self, result: &SpeedTestResult) -> Result<(), StoreError> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO speed_test_results (
                id, timestamp, agent_name, network_name, interface,
                download_mbps, upload_mbps, latency_ms, jitter_ms, packet_loss_pct,
                server_name, test_duration_ms, error
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                result.id.to_string(),
                result.timestamp.to_rfc3339(),
                result.agent_name,
                result.network_name,
                result.interface,
                result.download_mbps,
                result.upload_mbps,
                result.latency_ms,
                result.jitter_ms,
                result.packet_loss_pct,
                result.server_name,
                result.test_duration_ms.map(|v| v as i64),
                result.error,
            ],
        )?;
        Ok(())
    }

    pub fn list_speed_tests(
        &self,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<SpeedTestResult>, StoreError> {
        let conn = self.conn.lock();
        let mut sql = String::from(
            "SELECT id, timestamp, agent_name, network_name, interface,
                    download_mbps, upload_mbps, latency_ms, jitter_ms, packet_loss_pct,
                    server_name, test_duration_ms, error
             FROM speed_test_results WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(f) = from {
            sql.push_str(" AND timestamp >= ?");
            param_values.push(Box::new(f.to_rfc3339()));
        }
        if let Some(t) = to {
            sql.push_str(" AND timestamp <= ?");
            param_values.push(Box::new(t.to_rfc3339()));
        }
        sql.push_str(" ORDER BY timestamp DESC LIMIT ?");
        param_values.push(Box::new(limit as i64));

        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), parse_speedtest_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn speed_test_summary(
        &self,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    ) -> Result<serde_json::Value, StoreError> {
        let results = self.list_speed_tests(from, to, 10_000)?;
        if results.is_empty() {
            return Ok(serde_json::json!({
                "count": 0,
                "avg_download_mbps": null,
                "avg_upload_mbps": null,
                "avg_latency_ms": null,
            }));
        }
        let count = results.len();
        let avg = |f: fn(&SpeedTestResult) -> Option<f64>| -> Option<f64> {
            let vals: Vec<f64> = results.iter().filter_map(f).collect();
            if vals.is_empty() {
                None
            } else {
                Some(vals.iter().sum::<f64>() / vals.len() as f64)
            }
        };
        Ok(serde_json::json!({
            "count": count,
            "avg_download_mbps": avg(|r| r.download_mbps),
            "avg_upload_mbps": avg(|r| r.upload_mbps),
            "avg_latency_ms": avg(|r| r.latency_ms),
            "min_download_mbps": results.iter().filter_map(|r| r.download_mbps).reduce(f64::min),
            "max_download_mbps": results.iter().filter_map(|r| r.download_mbps).reduce(f64::max),
        }))
    }
}

fn parse_device_row(row: &rusqlite::Row<'_>) -> Result<Device, rusqlite::Error> {
    let kind_str: String = row.get(4)?;
    let os_str: String = row.get(5)?;
    let ports: Vec<u16> = serde_json::from_str(&row.get::<_, String>(11)?).unwrap_or_default();
    let services: Vec<String> = serde_json::from_str(&row.get::<_, String>(12)?).unwrap_or_default();
    Ok(Device {
        mac: MacAddress::from_str(&row.get::<_, String>(0)?).unwrap_or(MacAddress::new([0; 6])),
        current_ip: row
            .get::<_, Option<String>>(1)?
            .and_then(|s| s.parse().ok()),
        hostname: row.get(2)?,
        vendor: row.get(3)?,
        kind: parse_kind(&kind_str),
        os_hint: parse_os(&os_str),
        identity_id: row
            .get::<_, Option<String>>(6)?
            .and_then(|s| Uuid::parse_str(&s).ok()),
        user_label: row.get(7)?,
        first_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        last_seen: DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        online: row.get::<_, i32>(10)? != 0,
        open_ports: ports,
        mdns_services: services,
        confidence: row.get(13)?,
        inference_source: row.get(14)?,
        do_not_scan: row.get::<_, i32>(15)? != 0,
    })
}

fn parse_event_row(row: &rusqlite::Row<'_>) -> Result<EventRecord, rusqlite::Error> {
    let kind_str: String = row.get(1)?;
    Ok(EventRecord {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
        kind: parse_event_kind(&kind_str),
        timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        agent_name: row.get(3)?,
        network_name: row.get(4)?,
        device_mac: row
            .get::<_, Option<String>>(5)?
            .and_then(|s| MacAddress::from_str(&s).ok()),
        device_ip: row
            .get::<_, Option<String>>(6)?
            .and_then(|s| s.parse().ok()),
        message: row.get(7)?,
        details: row
            .get::<_, Option<String>>(8)?
            .and_then(|s| serde_json::from_str(&s).ok()),
    })
}

fn parse_speedtest_row(row: &rusqlite::Row<'_>) -> Result<SpeedTestResult, rusqlite::Error> {
    Ok(SpeedTestResult {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
        timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        agent_name: row.get(2)?,
        network_name: row.get(3)?,
        interface: row.get(4)?,
        download_mbps: row.get(5)?,
        upload_mbps: row.get(6)?,
        latency_ms: row.get(7)?,
        jitter_ms: row.get(8)?,
        packet_loss_pct: row.get(9)?,
        server_name: row.get(10)?,
        test_duration_ms: row
            .get::<_, Option<i64>>(11)?
            .map(|v| v as u64),
        error: row.get(12)?,
    })
}

fn parse_kind(s: &str) -> DeviceKind {
    match s {
        "router" => DeviceKind::Router,
        "mobile" => DeviceKind::Mobile,
        "desktop" => DeviceKind::Desktop,
        "iot" => DeviceKind::IoT,
        _ => DeviceKind::Unknown,
    }
}

fn parse_os(s: &str) -> OsHint {
    match s {
        "android" => OsHint::Android,
        "ios" => OsHint::Ios,
        "linux" => OsHint::Linux,
        "macos" => OsHint::MacOS,
        "windows" => OsHint::Windows,
        _ => OsHint::Unknown,
    }
}

fn parse_event_kind(s: &str) -> EventKind {
    match s {
        "network_down" => EventKind::NetworkDown,
        "network_restored" => EventKind::NetworkRestored,
        "device_joined" => EventKind::DeviceJoined,
        "device_left" => EventKind::DeviceLeft,
        "device_returned" => EventKind::DeviceReturned,
        "ip_changed" => EventKind::IpChanged,
        "kind_refined" => EventKind::KindRefined,
        "speed_test_completed" => EventKind::SpeedTestCompleted,
        _ => EventKind::DeviceJoined,
    }
}
