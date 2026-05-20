use crate::errors::{Result, TridentError};
use crate::transactions::batch::{BatchOp, WriteBatch};
use crate::types::{ColumnFamily, Key, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A named savepoint within a transaction.
///
/// Savepoints allow partial rollback within a transaction.
/// Rolling back to a savepoint undoes all operations after that point
/// but keeps the transaction alive.
#[derive(Clone, Debug)]
pub struct Savepoint {
    /// Name of the savepoint (user-provided or auto-generated).
    pub name: String,
    /// Index in the WriteBatch ops array at the time of savepoint creation.
    pub batch_index: usize,
    /// Snapshot of any additional state at savepoint time.
    pub timestamp_ms: u64,
}

/// Transaction with savepoint support.
///
/// Wraps a WriteBatch and adds the ability to create named savepoints
/// and roll back to them without aborting the entire transaction.
#[derive(Clone, Debug)]
pub struct SavepointTransaction {
    /// The accumulated write operations.
    batch: WriteBatch,
    /// Named savepoints, in creation order.
    savepoints: Vec<Savepoint>,
    /// Transaction ID.
    txn_id: u64,
}

impl SavepointTransaction {
    pub fn new(txn_id: u64) -> Self {
        Self {
            batch: WriteBatch::new(),
            savepoints: Vec::new(),
            txn_id,
        }
    }

    pub fn txn_id(&self) -> u64 {
        self.txn_id
    }

    /// Get the current write batch.
    pub fn batch(&self) -> &WriteBatch {
        &self.batch
    }

    /// Get the current write batch mutably.
    pub fn batch_mut(&mut self) -> &mut WriteBatch {
        &mut self.batch
    }

    /// Create a named savepoint.
    pub fn savepoint(&mut self, name: impl Into<String>) -> Savepoint {
        let sp = Savepoint {
            name: name.into(),
            batch_index: self.batch.len(),
            timestamp_ms: now_ms(),
        };
        self.savepoints.push(sp.clone());
        sp
    }

    /// Roll back to a named savepoint.
    ///
    /// Removes all operations after the savepoint but keeps the transaction alive.
    /// Returns the number of operations rolled back.
    pub fn rollback_to(&mut self, name: &str) -> Result<usize> {
        let sp_index = self
            .savepoints
            .iter()
            .rposition(|sp| sp.name == name)
            .ok_or_else(|| TridentError::TransactionConflict {
                cf: "default".into(),
                key: format!("savepoint '{}' not found", name),
            })?;

        let sp = &self.savepoints[sp_index];
        let rollback_count = self.batch.len() - sp.batch_index;

        // Truncate the batch to the savepoint position
        self.batch.truncate(sp.batch_index);

        // Remove this savepoint and all later ones
        self.savepoints.truncate(sp_index);

        Ok(rollback_count)
    }

    /// Release a named savepoint (discard it without rolling back).
    pub fn release_savepoint(&mut self, name: &str) -> Result<()> {
        let sp_index = self
            .savepoints
            .iter()
            .rposition(|sp| sp.name == name)
            .ok_or_else(|| TridentError::TransactionConflict {
                cf: "default".into(),
                key: format!("savepoint '{}' not found", name),
            })?;
        self.savepoints.remove(sp_index);
        Ok(())
    }

    /// List all active savepoints.
    pub fn savepoints(&self) -> &[Savepoint] {
        &self.savepoints
    }

    /// Check if a savepoint exists.
    pub fn has_savepoint(&self, name: &str) -> bool {
        self.savepoints.iter().any(|sp| sp.name == name)
    }

    /// Put a key-value pair.
    pub fn put(
        &mut self,
        cf: impl Into<ColumnFamily>,
        key: impl Into<Key>,
        value: impl Into<Value>,
    ) -> &mut Self {
        self.batch.put(cf, key, value);
        self
    }

    /// Put with default column family.
    pub fn put_default(&mut self, key: impl Into<Key>, value: impl Into<Value>) -> &mut Self {
        self.batch.put_default(key, value);
        self
    }

    /// Delete a key.
    pub fn delete(&mut self, cf: impl Into<ColumnFamily>, key: impl Into<Key>) -> &mut Self {
        self.batch.delete(cf, key);
        self
    }

    /// Delete with default column family.
    pub fn delete_default(&mut self, key: impl Into<Key>) -> &mut Self {
        self.batch.delete_default(key);
        self
    }

    /// Get the number of operations in the batch.
    pub fn len(&self) -> usize {
        self.batch.len()
    }

    /// Check if the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_savepoint_basic() {
        let mut txn = SavepointTransaction::new(1);

        txn.put_default(b"key1".to_vec(), b"value1".to_vec());
        txn.savepoint("sp1");
        txn.put_default(b"key2".to_vec(), b"value2".to_vec());
        txn.put_default(b"key3".to_vec(), b"value3".to_vec());

        assert_eq!(txn.len(), 3);

        let rolled = txn.rollback_to("sp1").unwrap();
        assert_eq!(rolled, 2);
        assert_eq!(txn.len(), 1);
    }

    #[test]
    fn test_multiple_savepoints() {
        let mut txn = SavepointTransaction::new(1);

        txn.put_default(b"a".to_vec(), b"1".to_vec());
        txn.savepoint("sp1");
        txn.put_default(b"b".to_vec(), b"2".to_vec());
        txn.savepoint("sp2");
        txn.put_default(b"c".to_vec(), b"3".to_vec());

        assert_eq!(txn.len(), 3);

        // Roll back to sp2 — loses "c"
        txn.rollback_to("sp2").unwrap();
        assert_eq!(txn.len(), 2);

        // Roll back to sp1 — loses "b"
        txn.rollback_to("sp1").unwrap();
        assert_eq!(txn.len(), 1);
    }

    #[test]
    fn test_savepoint_not_found() {
        let mut txn = SavepointTransaction::new(1);
        assert!(txn.rollback_to("nonexistent").is_err());
    }

    #[test]
    fn test_release_savepoint() {
        let mut txn = SavepointTransaction::new(1);
        txn.savepoint("sp1");
        assert!(txn.has_savepoint("sp1"));

        txn.release_savepoint("sp1").unwrap();
        assert!(!txn.has_savepoint("sp1"));
    }

    #[test]
    fn test_savepoint_preserves_earlier_ops() {
        let mut txn = SavepointTransaction::new(1);

        txn.put_default(b"keep".to_vec(), b"this".to_vec());
        txn.savepoint("sp1");
        txn.put_default(b"discard".to_vec(), b"this".to_vec());

        txn.rollback_to("sp1").unwrap();

        // Only the pre-savepoint op remains
        assert_eq!(txn.len(), 1);
        assert_eq!(txn.batch().ops()[0], BatchOp::Put {
            cf: ColumnFamily::default(),
            key: b"keep".to_vec(),
            value: b"this".to_vec(),
        });
    }
}
