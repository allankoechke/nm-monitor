use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("config error: {0}")]
    Config(String),
    #[error("invalid MAC address: {0}")]
    InvalidMac(String),
}
