use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("command failed to start: {0}")]
    Spawn(String),
    #[error("command timed out after {0}s: {1}")]
    Timeout(u64, String),
    #[error("command exited with status {status}: {stderr}")]
    NonZero { status: i32, stderr: String },
    #[error("could not parse output: {0}")]
    Parse(String),
    #[error("not supported: {0}")]
    Unsupported(String),
    #[error("database error: {0}")]
    Db(String),
    #[error("{0}")]
    Other(String),
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e.to_string())
    }
}
impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Parse(e.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
