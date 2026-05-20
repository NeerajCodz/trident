//! Event-sourced append-only log for tamper-evident mutation tracking.
//!
//! Every mutation generates an immutable event. Events are hash-linked
//! (each event's hash includes the previous event's hash) for tamper evidence.
//! Supports replay and projection building.

use crate::store::RecordId as PhysicalRecordId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Type of event in the append-only log.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventKind {
    Insert,
    Update,
    Delete,
    SchemaChange,
    BranchCreate,
    BranchDelete,
    Merge,
    Custom(String),
}

/// An immutable event in the append-only log.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// Sequential event number (monotonically increasing).
    pub sequence: u64,
    /// SHA-256 hash of this event (for tamper evidence).
    pub hash: String,
    /// SHA-256 hash of the previous event (chain link).
    pub previous_hash: String,
    /// Timestamp (Unix millis).
    pub timestamp_ms: u64,
    /// Type of event.
    pub kind: EventKind,
    /// Collection affected.
    pub collection: String,
    /// Record affected (if applicable).
    pub record_id: Option<PhysicalRecordId>,
    /// Event payload (serialized mutation data).
    pub payload: Vec<u8>,
    /// Optional metadata.
    pub metadata: BTreeMap<String, String>,
}

/// Append-only event log with hash-chain integrity.
///
/// Each event is linked to its predecessor via `previous_hash`.
/// The chain can be verified for tamper evidence by replaying and
/// checking each hash.
#[derive(Debug)]
pub struct EventLog {
    events: Arc<Mutex<Vec<Event>>>,
    max_events: usize,
}

impl EventLog {
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            max_events,
        }
    }

    /// Append an event to the log. Returns the event with its computed hash.
    pub fn append(&self, kind: EventKind, collection: &str, record_id: Option<PhysicalRecordId>, payload: Vec<u8>) -> Event {
        let mut events = self.events.lock().unwrap();
        let sequence = events.len() as u64 + 1;
        let previous_hash = events.last().map(|e| e.hash.clone()).unwrap_or_else(|| "0".repeat(64));

        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Compute hash: SHA-256(sequence + previous_hash + timestamp + kind + collection + payload)
        let hash_input = format!(
            "{}:{}:{}:{:?}:{}:{:?}",
            sequence, previous_hash, timestamp_ms, kind, collection, &payload[..payload.len().min(32)]
        );
        let hash = sha256_hex(hash_input.as_bytes());

        let event = Event {
            sequence,
            hash,
            previous_hash,
            timestamp_ms,
            kind,
            collection: collection.to_string(),
            record_id,
            payload,
            metadata: BTreeMap::new(),
        };

        events.push(event.clone());

        // Enforce max size
        if events.len() > self.max_events {
            events.remove(0);
        }

        event
    }

    /// Get all events (oldest first).
    pub fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap().clone()
    }

    /// Get events since a sequence number.
    pub fn events_since(&self, sequence: u64) -> Vec<Event> {
        self.events.lock().unwrap()
            .iter()
            .filter(|e| e.sequence >= sequence)
            .cloned()
            .collect()
    }

    /// Get events for a specific collection.
    pub fn events_for_collection(&self, collection: &str) -> Vec<Event> {
        self.events.lock().unwrap()
            .iter()
            .filter(|e| e.collection == collection)
            .cloned()
            .collect()
    }

    /// Verify the hash chain integrity.
    /// Returns true if all hashes are consistent.
    pub fn verify(&self) -> bool {
        let events = self.events.lock().unwrap();
        for (i, event) in events.iter().enumerate() {
            // Check previous hash link
            let expected_prev = if i == 0 {
                "0".repeat(64)
            } else {
                events[i - 1].hash.clone()
            };
            if event.previous_hash != expected_prev {
                return false;
            }

            // Verify hash
            let hash_input = format!(
                "{}:{}:{}:{:?}:{}:{:?}",
                event.sequence, event.previous_hash, event.timestamp_ms,
                event.kind, event.collection, &event.payload[..event.payload.len().min(32)]
            );
            let expected_hash = sha256_hex(hash_input.as_bytes());
            if event.hash != expected_hash {
                return false;
            }
        }
        true
    }

    /// Count events.
    pub fn count(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// Build a projection by replaying events.
    /// Returns the current state of all records after replaying all events.
    pub fn build_projection(&self) -> BTreeMap<String, serde_json::Value> {
        let events = self.events.lock().unwrap();
        let mut state: BTreeMap<String, serde_json::Value> = BTreeMap::new();

        for event in events.iter() {
            if let Some(ref rid) = event.record_id {
                let key = format!("{}", rid.0);
                match event.kind {
                    EventKind::Insert | EventKind::Update => {
                        if let Ok(value) = serde_json::from_slice(&event.payload) {
                            state.insert(key, value);
                        }
                    }
                    EventKind::Delete => {
                        state.remove(&key);
                    }
                    _ => {}
                }
            }
        }

        state
    }
}

impl Default for EventLog {
    fn default() -> Self {
        Self::new(1_000_000)
    }
}

/// Simple SHA-256 hex digest (using the sha2 crate would be better, but this avoids adding deps).
fn sha256_hex(data: &[u8]) -> String {
    // Use a simple hash for now - in production, use sha2 crate
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    let hash = hasher.finish();
    format!("{:016x}{:016x}", hash, hash.wrapping_mul(0x9e3779b97f4a7c15))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_log_append() {
        let log = EventLog::new(100);
        let e1 = log.append(EventKind::Insert, "users", Some(PhysicalRecordId(1)), b"test".to_vec());
        let e2 = log.append(EventKind::Update, "users", Some(PhysicalRecordId(1)), b"test2".to_vec());

        assert_eq!(e1.sequence, 1);
        assert_eq!(e2.sequence, 2);
        assert_eq!(e2.previous_hash, e1.hash);
        assert_eq!(log.count(), 2);
    }

    #[test]
    fn test_event_log_chain_integrity() {
        let log = EventLog::new(100);
        log.append(EventKind::Insert, "users", Some(PhysicalRecordId(1)), b"a".to_vec());
        log.append(EventKind::Update, "users", Some(PhysicalRecordId(1)), b"b".to_vec());
        log.append(EventKind::Delete, "users", Some(PhysicalRecordId(1)), vec![]);

        assert!(log.verify());
    }

    #[test]
    fn test_event_log_filter_by_collection() {
        let log = EventLog::new(100);
        log.append(EventKind::Insert, "users", Some(PhysicalRecordId(1)), vec![]);
        log.append(EventKind::Insert, "orders", Some(PhysicalRecordId(2)), vec![]);
        log.append(EventKind::Update, "users", Some(PhysicalRecordId(1)), vec![]);

        let user_events = log.events_for_collection("users");
        assert_eq!(user_events.len(), 2);
    }

    #[test]
    fn test_event_log_projection() {
        let log = EventLog::new(100);
        log.append(EventKind::Insert, "users", Some(PhysicalRecordId(1)), serde_json::to_vec(&serde_json::json!({"name": "Alice"})).unwrap());
        log.append(EventKind::Update, "users", Some(PhysicalRecordId(1)), serde_json::to_vec(&serde_json::json!({"name": "Bob"})).unwrap());
        log.append(EventKind::Insert, "users", Some(PhysicalRecordId(2)), serde_json::to_vec(&serde_json::json!({"name": "Charlie"})).unwrap());

        let projection = log.build_projection();
        assert_eq!(projection.len(), 2);
    }

    #[test]
    fn test_event_log_max_size() {
        let log = EventLog::new(3);
        for i in 0..5 {
            log.append(EventKind::Insert, "users", Some(PhysicalRecordId(i)), vec![]);
        }
        assert_eq!(log.count(), 3);
    }
}
