use crate::error::StoreError;
use refinery::embed_migrations;
use rusqlite::Connection;

embed_migrations!("migrations");

pub fn run_migrations(conn: &mut Connection) -> Result<(), StoreError> {
    migrations::runner()
        .run(conn)
        .map_err(|e| StoreError::Migration(e.to_string()))?;
    Ok(())
}
