use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("plan draft {0} is already published")]
    PlanPublished(i64),
    #[error("plan draft {0} is not reserved for publishing")]
    PlanNotReserved(i64),
}

pub type Result<T> = std::result::Result<T, StoreError>;
