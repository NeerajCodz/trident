use crate::errors::{PraxisError, Result};
use crate::transactions::batch::WriteBatch;
use crate::types::{ColumnFamily, Key, Value};

/// Nested transaction support.
///
/// A nested (child) transaction inherits the parent's snapshot and
/// accumulates its own writes. On commit, the child's writes are
/// merged into the parent. On rollback, the child's writes are discarded
/// without affecting the parent.
///
/// Nesting is unlimited — a child can itself have children.
///
/// ## Example
///
/// ```ignore
/// let mut parent = NestedTransaction::new(1);
/// parent.put_default(b"key1", b"value1");
///
/// let mut child = parent.begin_child();
/// child.put_default(b"key2", b"value2");
/// child.commit_to_parent(); // "key2" now visible in parent
///
/// parent.put_default(b"key3", b"value3");
/// parent.rollback(); // discards key1, key2, key3
/// ```
#[derive(Clone, Debug)]
pub struct NestedTransaction {
    /// Transaction ID.
    txn_id: u64,
    /// Write operations accumulated by this transaction.
    batch: WriteBatch,
    /// Child transactions (for nested commit semantics).
    children: Vec<NestedChild>,
    /// Parent snapshot (inherited by children).
    _parent_snapshot: Option<u64>,
    /// Whether this transaction has been committed or rolled back.
    state: TransactionState,
}

#[derive(Clone, Debug)]
struct NestedChild {
    _txn_id: u64,
    batch: WriteBatch,
    committed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransactionState {
    Active,
    Committed,
    RolledBack,
}

impl NestedTransaction {
    /// Create a new root-level nested transaction.
    pub fn new(txn_id: u64) -> Self {
        Self {
            txn_id,
            batch: WriteBatch::new(),
            children: Vec::new(),
            _parent_snapshot: None,
            state: TransactionState::Active,
        }
    }

    /// Create a child transaction that inherits this transaction's context.
    pub fn begin_child(&self) -> NestedTransaction {
        NestedTransaction {
            txn_id: self.txn_id, // inherit parent txn ID
            batch: WriteBatch::new(),
            children: Vec::new(),
            _parent_snapshot: None,
            state: TransactionState::Active,
        }
    }

    /// Commit a child transaction — its writes become visible to the parent.
    /// The child's own writes AND its committed children's writes are merged.
    pub fn commit_child(&mut self, mut child: NestedTransaction) -> Result<()> {
        if child.state != TransactionState::Active {
            return Err(PraxisError::TransactionConflict {
                cf: "default".into(),
                key: "child transaction is not active".into(),
            });
        }

        // Merge child's committed grandchildren into the child's batch
        let mut merged_batch = child.batch;
        for grandchild in child.children.drain(..) {
            if grandchild.committed {
                for op in grandchild.batch.ops() {
                    merged_batch.push_op(op.clone());
                }
            }
        }

        self.children.push(NestedChild {
            _txn_id: child.txn_id,
            batch: merged_batch,
            committed: true,
        });

        Ok(())
    }

    /// Roll back a child transaction — its writes are discarded.
    pub fn rollback_child(&mut self, child: NestedTransaction) {
        // Just drop the child — its writes are never merged
        let _ = child;
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

    /// Commit this transaction.
    /// Returns the merged write batch (this transaction + all committed children).
    pub fn commit(mut self) -> Result<WriteBatch> {
        self.state = TransactionState::Committed;

        // Merge all committed children's batches into this one
        let mut merged = self.batch;
        for child in self.children.drain(..) {
            if child.committed {
                for op in child.batch.ops() {
                    merged.push_op(op.clone());
                }
            }
        }

        Ok(merged)
    }

    /// Roll back this transaction — discard all writes.
    pub fn rollback(mut self) {
        self.state = TransactionState::RolledBack;
        self.batch = WriteBatch::new();
        self.children.clear();
    }

    /// Get the current write batch (without children).
    pub fn batch(&self) -> &WriteBatch {
        &self.batch
    }

    /// Get the total number of operations (this + committed children).
    pub fn total_ops(&self) -> usize {
        let child_ops: usize = self
            .children
            .iter()
            .filter(|c| c.committed)
            .map(|c| c.batch.len())
            .sum();
        self.batch.len() + child_ops
    }

    /// Get the transaction ID.
    pub fn txn_id(&self) -> u64 {
        self.txn_id
    }

    /// Check if this transaction is still active.
    pub fn is_active(&self) -> bool {
        self.state == TransactionState::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nested_commit() {
        let mut parent = NestedTransaction::new(1);
        parent.put_default(b"key1".to_vec(), b"val1".to_vec());

        let mut child = parent.begin_child();
        child.put_default(b"key2".to_vec(), b"val2".to_vec());
        parent.commit_child(child).unwrap();

        let merged = parent.commit().unwrap();
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_nested_rollback_child() {
        let mut parent = NestedTransaction::new(1);
        parent.put_default(b"key1".to_vec(), b"val1".to_vec());

        let child = parent.begin_child();
        let mut child2 = child;
        child2.put_default(b"key2".to_vec(), b"val2".to_vec());
        parent.rollback_child(child2);

        let merged = parent.commit().unwrap();
        assert_eq!(merged.len(), 1); // only parent's write
    }

    #[test]
    fn test_nested_rollback_parent() {
        let mut parent = NestedTransaction::new(1);
        parent.put_default(b"key1".to_vec(), b"val1".to_vec());

        let mut child = parent.begin_child();
        child.put_default(b"key2".to_vec(), b"val2".to_vec());
        parent.commit_child(child).unwrap();

        parent.put_default(b"key3".to_vec(), b"val3".to_vec());

        parent.rollback(); // discard everything
        // No way to check after rollback since it consumes self,
        // but it should not panic
    }

    #[test]
    fn test_multi_level_nesting() {
        let mut root = NestedTransaction::new(1);
        root.put_default(b"root".to_vec(), b"1".to_vec());

        let mut level1 = root.begin_child();
        level1.put_default(b"level1".to_vec(), b"2".to_vec());

        let mut level2 = level1.begin_child();
        level2.put_default(b"level2".to_vec(), b"3".to_vec());
        level1.commit_child(level2).unwrap();

        root.commit_child(level1).unwrap();

        let merged = root.commit().unwrap();
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn test_total_ops() {
        let mut parent = NestedTransaction::new(1);
        parent.put_default(b"a".to_vec(), b"1".to_vec());

        let mut child = parent.begin_child();
        child.put_default(b"b".to_vec(), b"2".to_vec());
        child.put_default(b"c".to_vec(), b"3".to_vec());
        parent.commit_child(child).unwrap();

        assert_eq!(parent.total_ops(), 3);
    }
}
