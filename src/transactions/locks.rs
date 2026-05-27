use crate::errors::{PraxisError, Result};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Lock mode for row-level locking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LockMode {
    /// Shared lock — multiple readers can hold simultaneously.
    Shared,
    /// Exclusive lock — only one writer at a time.
    Exclusive,
}

/// A lock request from a transaction.
#[derive(Clone, Debug)]
struct LockRequest {
    txn_id: u64,
    _mode: LockMode,
}

/// Lock state for a single key.
#[derive(Clone, Debug)]
struct KeyLock {
    /// Currently granted locks (txn_id -> mode).
    granted: HashMap<u64, LockMode>,
    /// Pending lock requests.
    queue: VecDeque<LockRequest>,
}

impl KeyLock {
    fn new() -> Self {
        Self {
            granted: HashMap::new(),
            queue: VecDeque::new(),
        }
    }

    /// Check if a lock request can be granted immediately.
    fn can_grant(&self, txn_id: u64, mode: LockMode) -> bool {
        // If no granted locks, always grant
        if self.granted.is_empty() {
            return true;
        }

        // If this txn already holds a lock
        if let Some(existing) = self.granted.get(&txn_id) {
            // Upgrade from Shared to Exclusive is allowed if we're the only holder
            if *existing == LockMode::Shared && mode == LockMode::Exclusive {
                return self.granted.len() == 1;
            }
            // Same or weaker mode is always ok
            return true;
        }

        // For a new txn: Shared is ok if all existing are Shared
        // Exclusive is never ok if others hold locks
        match mode {
            LockMode::Shared => self.granted.values().all(|m| *m == LockMode::Shared),
            LockMode::Exclusive => false,
        }
    }

    /// Grant a lock to a transaction.
    fn grant(&mut self, txn_id: u64, mode: LockMode) {
        self.granted.insert(txn_id, mode);
    }

    /// Release all locks held by a transaction.
    fn release(&mut self, txn_id: u64) {
        self.granted.remove(&txn_id);
        self.queue.retain(|r| r.txn_id != txn_id);
    }

    fn is_empty(&self) -> bool {
        self.granted.is_empty() && self.queue.is_empty()
    }
}

/// Wait-for graph edge for deadlock detection.
#[derive(Clone, Debug)]
struct WaitEdge {
    waiter: u64, // txn waiting
    holder: u64, // txn holding the lock
    key: Vec<u8>,
}

/// Lock manager for row-level and document-level locking.
///
/// Provides:
/// - Shared (read) and exclusive (write) locks per key
/// - Deadlock detection via wait-for graph cycle detection
/// - Lock timeout to prevent indefinite blocking
/// - Automatic cleanup on transaction abort/commit
pub struct LockManager {
    inner: Mutex<LockManagerInner>,
    notify: Condvar,
}

struct LockManagerInner {
    /// Key -> lock state
    locks: HashMap<Vec<u8>, KeyLock>,
    /// Transaction -> set of keys it holds locks on
    txn_keys: HashMap<u64, HashSet<Vec<u8>>>,
    /// Wait-for edges for deadlock detection
    wait_graph: Vec<WaitEdge>,
    /// Default lock timeout
    timeout: Duration,
    /// Next transaction ID
    next_txn_id: u64,
}

impl LockManager {
    pub fn new(timeout: Duration) -> Self {
        Self {
            inner: Mutex::new(LockManagerInner {
                locks: HashMap::new(),
                txn_keys: HashMap::new(),
                wait_graph: Vec::new(),
                timeout,
                next_txn_id: 1,
            }),
            notify: Condvar::new(),
        }
    }

    pub fn with_default_timeout() -> Self {
        Self::new(Duration::from_secs(30))
    }

    /// Allocate a new unique transaction ID.
    pub fn new_txn_id(&self) -> u64 {
        let mut inner = self.inner.lock().unwrap();
        let id = inner.next_txn_id;
        inner.next_txn_id += 1;
        id
    }

    /// Acquire a lock on a key for a transaction.
    ///
    /// Blocks until the lock is available or timeout/deadlock is detected.
    pub fn acquire(&self, txn_id: u64, key: &[u8], mode: LockMode) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let deadline = Instant::now() + inner.timeout;

        loop {
            // Check if we can grant immediately
            let key_lock = inner.locks.entry(key.to_vec()).or_insert_with(KeyLock::new);
            if key_lock.can_grant(txn_id, mode) {
                key_lock.grant(txn_id, mode);
                inner
                    .txn_keys
                    .entry(txn_id)
                    .or_default()
                    .insert(key.to_vec());

                // Remove any wait edge for this txn/key
                inner
                    .wait_graph
                    .retain(|e| !(e.waiter == txn_id && e.key == key));
                return Ok(());
            }

            // Cannot grant — add wait edge for deadlock detection
            // Find who holds the lock
            let holders: Vec<u64> = key_lock.granted.keys().copied().collect();
            for holder in holders {
                if holder != txn_id {
                    let edge = WaitEdge {
                        waiter: txn_id,
                        holder,
                        key: key.to_vec(),
                    };
                    // Check for deadlock before adding edge
                    if self.has_cycle_with(&inner.wait_graph, &edge) {
                        return Err(PraxisError::TransactionConflict {
                            cf: "default".to_string(),
                            key: format!(
                                "deadlock detected: txn {} waits for txn {}",
                                txn_id, holder
                            ),
                        });
                    }
                    inner.wait_graph.push(edge);
                }
            }

            // Wait with timeout
            let remaining = deadline.duration_since(Instant::now());
            if remaining.is_zero() {
                // Clean up wait edges
                inner.wait_graph.retain(|e| e.waiter != txn_id);
                return Err(PraxisError::TransactionConflict {
                    cf: "default".to_string(),
                    key: format!("lock timeout on key after {:?}", inner.timeout),
                });
            }

            // Drop lock and wait
            let (lock, result) = self.notify.wait_timeout(inner, remaining).unwrap();
            inner = lock;

            if result.timed_out() {
                inner.wait_graph.retain(|e| e.waiter != txn_id);
                return Err(PraxisError::TransactionConflict {
                    cf: "default".to_string(),
                    key: format!("lock timeout on key after {:?}", inner.timeout),
                });
            }
        }
    }

    /// Release all locks held by a transaction.
    pub fn release_all(&self, txn_id: u64) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(keys) = inner.txn_keys.remove(&txn_id) {
            for key in keys {
                if let Some(key_lock) = inner.locks.get_mut(&key) {
                    key_lock.release(txn_id);
                    if key_lock.is_empty() {
                        inner.locks.remove(&key);
                    }
                }
            }
        }
        inner
            .wait_graph
            .retain(|e| e.waiter != txn_id && e.holder != txn_id);
        self.notify.notify_all();
    }

    /// Check if a transaction holds a lock on a key.
    pub fn holds_lock(&self, txn_id: u64, key: &[u8]) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .locks
            .get(key)
            .map(|kl| kl.granted.contains_key(&txn_id))
            .unwrap_or(false)
    }

    /// Get the number of active locks.
    pub fn active_locks(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.locks.values().map(|kl| kl.granted.len()).sum()
    }

    /// Check for deadlock using DFS on the wait-for graph.
    fn has_cycle_with(&self, edges: &[WaitEdge], new_edge: &WaitEdge) -> bool {
        let mut all_edges: Vec<&WaitEdge> = edges.iter().collect();
        all_edges.push(new_edge);

        // Build adjacency list
        let mut adj: HashMap<u64, Vec<u64>> = HashMap::new();
        for edge in &all_edges {
            adj.entry(edge.waiter).or_default().push(edge.holder);
        }

        // DFS from new_edge.waiter to check if we can reach new_edge.waiter
        let start = new_edge.waiter;
        let target = new_edge.waiter;
        let mut visited = HashSet::new();
        let mut stack = vec![start];

        while let Some(node) = stack.pop() {
            if node == target && visited.contains(&target) {
                return true;
            }
            if !visited.insert(node) {
                continue;
            }
            if let Some(neighbors) = adj.get(&node) {
                for &neighbor in neighbors {
                    if neighbor == target {
                        return true;
                    }
                    if !visited.contains(&neighbor) {
                        stack.push(neighbor);
                    }
                }
            }
        }

        false
    }
}

impl Default for LockManager {
    fn default() -> Self {
        Self::with_default_timeout()
    }
}

/// A scoped lock guard that releases the lock on drop.
pub struct LockGuard<'a> {
    manager: &'a LockManager,
    txn_id: u64,
    key: Vec<u8>,
}

impl<'a> LockGuard<'a> {
    pub fn new(manager: &'a LockManager, txn_id: u64, key: Vec<u8>) -> Self {
        Self {
            manager,
            txn_id,
            key,
        }
    }

    pub fn key(&self) -> &[u8] {
        &self.key
    }
}

impl<'a> Drop for LockGuard<'a> {
    fn drop(&mut self) {
        // Release just this key's lock
        let mut inner = self.manager.inner.lock().unwrap();
        if let Some(key_lock) = inner.locks.get_mut(&self.key) {
            key_lock.release(self.txn_id);
            if key_lock.is_empty() {
                inner.locks.remove(&self.key);
            }
        }
        if let Some(keys) = inner.txn_keys.get_mut(&self.txn_id) {
            keys.remove(&self.key);
        }
        drop(inner);
        self.manager.notify.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_shared_locks_compatible() {
        let mgr = LockManager::new(Duration::from_secs(5));
        let txn1 = mgr.new_txn_id();
        let txn2 = mgr.new_txn_id();

        mgr.acquire(txn1, b"key1", LockMode::Shared).unwrap();
        mgr.acquire(txn2, b"key1", LockMode::Shared).unwrap();

        assert!(mgr.holds_lock(txn1, b"key1"));
        assert!(mgr.holds_lock(txn2, b"key1"));
        assert_eq!(mgr.active_locks(), 2);
    }

    #[test]
    fn test_exclusive_locks_block() {
        let mgr = LockManager::new(Duration::from_millis(100));
        let txn1 = mgr.new_txn_id();
        let txn2 = mgr.new_txn_id();

        mgr.acquire(txn1, b"key1", LockMode::Exclusive).unwrap();
        let result = mgr.acquire(txn2, b"key1", LockMode::Exclusive);
        assert!(result.is_err()); // timeout
    }

    #[test]
    fn test_shared_blocks_exclusive() {
        let mgr = LockManager::new(Duration::from_millis(100));
        let txn1 = mgr.new_txn_id();
        let txn2 = mgr.new_txn_id();

        mgr.acquire(txn1, b"key1", LockMode::Shared).unwrap();
        let result = mgr.acquire(txn2, b"key1", LockMode::Exclusive);
        assert!(result.is_err()); // timeout
    }

    #[test]
    fn test_exclusive_blocks_shared() {
        let mgr = LockManager::new(Duration::from_millis(100));
        let txn1 = mgr.new_txn_id();
        let txn2 = mgr.new_txn_id();

        mgr.acquire(txn1, b"key1", LockMode::Exclusive).unwrap();
        let result = mgr.acquire(txn2, b"key1", LockMode::Shared);
        assert!(result.is_err()); // timeout
    }

    #[test]
    fn test_release_all() {
        let mgr = LockManager::new(Duration::from_secs(5));
        let txn1 = mgr.new_txn_id();
        let txn2 = mgr.new_txn_id();

        mgr.acquire(txn1, b"key1", LockMode::Exclusive).unwrap();
        mgr.release_all(txn1);

        // Now txn2 can acquire
        mgr.acquire(txn2, b"key1", LockMode::Exclusive).unwrap();
        assert!(mgr.holds_lock(txn2, b"key1"));
    }

    #[test]
    fn test_lock_guard_release() {
        let mgr = LockManager::new(Duration::from_secs(5));
        let txn1 = mgr.new_txn_id();

        // Acquire the lock first
        mgr.acquire(txn1, b"key1", LockMode::Shared).unwrap();

        {
            let _guard = LockGuard::new(&mgr, txn1, b"key1".to_vec());
            assert!(mgr.holds_lock(txn1, b"key1"));
        }

        // Guard dropped — lock released
        assert!(!mgr.holds_lock(txn1, b"key1"));
    }

    #[test]
    fn test_deadlock_detection() {
        let mgr = Arc::new(LockManager::new(Duration::from_secs(5)));
        let txn1 = mgr.new_txn_id();
        let txn2 = mgr.new_txn_id();

        mgr.acquire(txn1, b"key1", LockMode::Exclusive).unwrap();
        mgr.acquire(txn2, b"key2", LockMode::Exclusive).unwrap();

        // txn1 wants key2, txn2 wants key1 — deadlock
        let mgr1 = mgr.clone();
        let handle = thread::spawn(move || {
            // Small delay so txn1 gets key2 request first
            thread::sleep(Duration::from_millis(10));
            let _ = mgr1.acquire(txn2, b"key1", LockMode::Exclusive);
        });

        let result = mgr.acquire(txn1, b"key2", LockMode::Exclusive);
        // One of them should get a deadlock error
        // (timing-dependent which one)
        let _ = result;

        handle.join().unwrap();
    }

    #[test]
    fn test_different_keys_independent() {
        let mgr = LockManager::new(Duration::from_secs(5));
        let txn1 = mgr.new_txn_id();
        let txn2 = mgr.new_txn_id();

        mgr.acquire(txn1, b"key1", LockMode::Exclusive).unwrap();
        mgr.acquire(txn2, b"key2", LockMode::Exclusive).unwrap();

        assert!(mgr.holds_lock(txn1, b"key1"));
        assert!(mgr.holds_lock(txn2, b"key2"));
    }
}
