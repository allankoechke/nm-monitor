use crate::channels::{DesktopChannel, NtfyChannel, WebhookChannel};
use crate::fcm::FcmChannel;
use async_trait::async_trait;
use nm_core::config::{NotificationsConfig, resolve_secret};
use nm_core::notify::{format_notification, NotificationPayload};
use thiserror::Error;
use tracing::{error, info};

#[derive(Debug, Error)]
pub enum NotifyError {
    #[error("notification failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait NotifyChannel: Send + Sync {
    fn name(&self) -> &'static str;
    async fn send(&self, payload: &NotificationPayload) -> Result<(), NotifyError>;
}

pub struct NotificationDispatcher {
    channels: Vec<Box<dyn NotifyChannel>>,
}

impl NotificationDispatcher {
    pub fn from_config(config: &NotificationsConfig) -> Self {
        let mut channels: Vec<Box<dyn NotifyChannel>> = Vec::new();
        if config.ntfy.enabled {
            channels.push(Box::new(NtfyChannel::new(
                config.ntfy.server.clone(),
                config.ntfy.topic.clone(),
            )));
        }
        if config.webhook.enabled && !config.webhook.url.is_empty() {
            channels.push(Box::new(WebhookChannel::new(
                config.webhook.url.clone(),
                resolve_secret(&config.webhook.secret),
            )));
        }
        if config.desktop.enabled {
            channels.push(Box::new(DesktopChannel));
        }
        if config.fcm.enabled {
            channels.push(Box::new(FcmChannel::new(
                config.fcm.project_id.clone(),
                config.fcm.credentials_path.clone(),
            )));
        }
        Self { channels }
    }

    pub async fn dispatch(&self, payload: &NotificationPayload) {
        let (title, body) = format_notification(payload);
        info!(title = %title, "dispatching notification");
        for channel in &self.channels {
            if let Err(e) = channel.send(payload).await {
                error!(channel = channel.name(), error = %e, "notification channel failed");
            }
        }
        let _ = body;
    }
}
