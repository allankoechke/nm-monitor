use async_trait::async_trait;
use hmac::{Hmac, Mac};
use nm_core::notify::NotificationPayload;
use reqwest::Client;
use sha2::Sha256;
use tracing::warn;

use crate::dispatcher::{NotifyChannel, NotifyError};

pub struct NtfyChannel {
    server: String,
    topic: String,
    client: Client,
}

impl NtfyChannel {
    pub fn new(server: String, topic: String) -> Self {
        Self {
            server,
            topic,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl NotifyChannel for NtfyChannel {
    fn name(&self) -> &'static str {
        "ntfy"
    }

    async fn send(&self, payload: &NotificationPayload) -> Result<(), NotifyError> {
        let (title, body) = nm_core::notify::format_notification(payload);
        let url = format!(
            "{}/{}",
            self.server.trim_end_matches('/'),
            self.topic
        );
        self.client
            .post(&url)
            .header("Title", title)
            .header("Tags", "network,wifi")
            .body(body)
            .send()
            .await
            .map_err(|e| NotifyError::Failed(e.to_string()))?
            .error_for_status()
            .map_err(|e| NotifyError::Failed(e.to_string()))?;
        Ok(())
    }
}

pub struct WebhookChannel {
    url: String,
    secret: Option<String>,
    client: Client,
}

impl WebhookChannel {
    pub fn new(url: String, secret: Option<String>) -> Self {
        Self {
            url,
            secret,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl NotifyChannel for WebhookChannel {
    fn name(&self) -> &'static str {
        "webhook"
    }

    async fn send(&self, payload: &NotificationPayload) -> Result<(), NotifyError> {
        let body = serde_json::to_string(payload)
            .map_err(|e| NotifyError::Failed(e.to_string()))?;
        let mut req = self.client.post(&self.url).header("Content-Type", "application/json");
        if let Some(secret) = &self.secret {
            let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                .map_err(|e| NotifyError::Failed(e.to_string()))?;
            mac.update(body.as_bytes());
            let sig = hex::encode(mac.finalize().into_bytes());
            req = req.header("X-Signature-SHA256", sig);
        }
        req.body(body)
            .send()
            .await
            .map_err(|e| NotifyError::Failed(e.to_string()))?
            .error_for_status()
            .map_err(|e| NotifyError::Failed(e.to_string()))?;
        Ok(())
    }
}

pub struct DesktopChannel;

#[async_trait]
impl NotifyChannel for DesktopChannel {
    fn name(&self) -> &'static str {
        "desktop"
    }

    async fn send(&self, payload: &NotificationPayload) -> Result<(), NotifyError> {
        let (title, body) = nm_core::notify::format_notification(payload);
        tokio::task::spawn_blocking(move || {
            if let Err(e) = notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                .show()
            {
                warn!("desktop notification failed: {e}");
            }
        })
        .await
        .map_err(|e| NotifyError::Failed(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use nm_core::event::EventKind;

    #[test]
    fn webhook_payload_includes_agent_and_network() {
        let payload = NotificationPayload {
            agent_name: "Home Pi".into(),
            network_name: None,
            kind: EventKind::NetworkDown,
            title: "Network is down".into(),
            body: "gateway unreachable".into(),
            timestamp: Utc::now(),
            device_name: None,
            device_ip: None,
            gateway: None,
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["agent_name"], "Home Pi");
        assert!(json["network_name"].is_null());
    }
}
