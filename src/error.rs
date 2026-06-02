use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("scope not found")]
    ScopeNotFound,

    #[error("scope already exists")]
    ScopeAlreadyExists,

    #[error("invalid pattern: {0}")]
    InvalidPattern(String),

    #[error("no active scope")]
    NoActiveScope,

    #[error("file not tracked: {0}")]
    FileNotTracked(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),
}

impl Error {
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::ScopeNotFound | Error::NoActiveScope | Error::FileNotTracked(_) => 2,
            Error::ScopeAlreadyExists => 3,
            _ => 1,
        }
    }
}
