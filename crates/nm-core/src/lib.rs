pub mod config;
pub mod device;
pub mod error;
pub mod event;
pub mod identity;
pub mod notify;
pub mod speedtest;

pub use config::AppConfig;
pub use device::{Device, DeviceKind, DeviceSnapshot, OsHint};
pub use error::CoreError;
pub use event::{EventKind, EventRecord};
pub use identity::Identity;
pub use notify::{format_notification, NotificationPayload};
pub use speedtest::{SpeedTestBackendKind, SpeedTestConfig, SpeedTestResult};
