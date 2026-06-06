use async_trait::async_trait;
use nm_core::notify::NotificationPayload;
use tracing::warn;

use crate::dispatcher::{NotifyChannel, NotifyError};

/// FCM push notification channel stub for future Android companion app integration.
pub struct FcmChannel {
    project_id: Option<String>,
    credentials_path: Option<String>,
}

impl FcmChannel {
    pub fn new(project_id: Option<String>, credentials_path: Option<String>) -> Self {
        Self {
            project_id,
            credentials_path,
        }
    }
}

#[async_trait]
impl NotifyChannel for FcmChannel {
    fn name(&self) -> &'static str {
        "fcm"
    }

    async fn send(&self, payload: &NotificationPayload) -> Result<(), NotifyError> {
        warn!(
            project_id = ?self.project_id,
            credentials_path = ?self.credentials_path,
            event = ?payload.kind,
            "FCM channel is a stub — implement with Firebase Admin SDK and companion Android app"
        );
        Err(NotifyError::Failed(
            "FCM not yet implemented — use ntfy or webhook for Android notifications".into(),
        ))
    }
}
