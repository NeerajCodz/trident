use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum TridentError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("toml error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("toml error: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("corrupt data in {path}: {reason}")]
    Corrupt { path: PathBuf, reason: String },
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("key not found")]
    KeyNotFound,
    #[error("compare-and-swap failed")]
    CompareAndSwapFailed,
    #[error("transaction conflict on key in column family {cf}: {key}")]
    TransactionConflict { cf: String, key: String },
    #[error("unknown column family: {0}")]
    UnknownColumnFamily(String),
    #[error("column family already exists: {0}")]
    ColumnFamilyExists(String),
    #[error("cannot drop the default column family")]
    CannotDropDefaultColumnFamily,
    #[error("unsupported accelerator backend: {0}")]
    UnsupportedAccelerator(String),
    #[error("server error: {0}")]
    Server(String),
}

pub type Result<T> = std::result::Result<T, TridentError>;
