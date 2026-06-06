use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub id: Uuid,
    pub display_name: String,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
}
