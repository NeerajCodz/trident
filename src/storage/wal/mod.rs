pub mod record;
pub mod writer;

pub use record::WalRecord;
pub use writer::Wal;

// Aliases for backward compatibility with store module
pub type StorageWal = Wal;
pub type StorageWalEntry = WalRecord;

// Placeholder types for compatibility
#[derive(Clone, Debug, Default)]
pub enum StorageWalOperation {
    #[default]
    Put,
    Delete,
}

#[derive(Clone, Debug, Default)]
pub struct StorageWalOptions;

impl Wal {
    /// Opens a WAL file with the storage engine interface (for backward compat)
    /// Note: This is a simplified wrapper that ignores segment_id argument
    pub fn open_simple(path: impl Into<std::path::PathBuf>) -> crate::errors::Result<Self> {
        use crate::config::WalSyncPolicy;
        Wal::open(path, 0, WalSyncPolicy::EveryBatch)
    }
    
    /// Open with options (simplified)
    pub fn open_with_options(path: impl Into<std::path::PathBuf>, _opts: StorageWalOptions) -> crate::errors::Result<Self> {
        use crate::config::WalSyncPolicy;
        Wal::open(path, 0, WalSyncPolicy::EveryBatch)
    }
    
    /// Append multiple records in batch
    pub fn append_batch(&mut self, records: Vec<WalRecord>) -> crate::errors::Result<()> {
        for record in records {
            self.append(&record)?;
        }
        Ok(())
    }
}
