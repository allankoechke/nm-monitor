use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid data: {0}")]
    InvalidData(String),
}
