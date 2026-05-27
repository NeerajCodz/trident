//! High-level document/record types for the query and execution layer.
//!
//! This module provides the multi-model Record type that supports:
//! - Document attributes (JSON values)
//! - Vector embeddings
//! - Graph edges
//! - Labels
//! - Timestamps

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Logical record identifier: `collection:key`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordId {
    pub collection: String,
    pub key: String,
}

impl RecordId {
    pub fn new(collection: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            key: key.into(),
        }
    }

    pub fn storage_key(&self) -> String {
        format!("{}:{}", self.collection, self.key)
    }
}

impl std::fmt::Display for RecordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.collection, self.key)
    }
}

/// A multi-model record supporting documents, vectors, and graph edges.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub id: RecordId,
    pub labels: Vec<String>,
    pub attributes: BTreeMap<String, Value>,
    pub vectors: BTreeMap<String, Vec<f32>>,
    pub edges: Vec<Edge>,
    pub created_at_ms: u128,
    pub updated_at_ms: u128,
}

impl Record {
    pub fn new(collection: impl Into<String>, key: impl Into<String>) -> Self {
        let now = now_ms();
        Self {
            id: RecordId::new(collection, key),
            labels: Vec::new(),
            attributes: BTreeMap::new(),
            vectors: BTreeMap::new(),
            edges: Vec::new(),
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    pub fn with_attribute(mut self, name: impl Into<String>, value: Value) -> Self {
        self.attributes.insert(name.into(), value);
        self.updated_at_ms = now_ms();
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        if !self.labels.contains(&label) {
            self.labels.push(label);
        }
        self.updated_at_ms = now_ms();
        self
    }

    pub fn with_vector(mut self, name: impl Into<String>, vector: Vec<f32>) -> Self {
        self.vectors.insert(name.into(), vector);
        self.updated_at_ms = now_ms();
        self
    }

    pub fn with_edge(self, label: impl Into<String>, target: RecordId) -> Self {
        self.with_relationship(label, target)
    }

    pub fn with_relationship(
        mut self,
        relationship_type: impl Into<String>,
        target: RecordId,
    ) -> Self {
        self.edges.push(Edge {
            relationship_type: relationship_type.into(),
            target,
            properties: BTreeMap::new(),
        });
        self.updated_at_ms = now_ms();
        self
    }
}

/// A graph edge connecting two records.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Edge {
    pub relationship_type: String,
    pub target: RecordId,
    pub properties: BTreeMap<String, Value>,
}

impl Edge {
    pub fn label(&self) -> &str {
        &self.relationship_type
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// Bridge between logical document::RecordId (collection:key) and physical store::RecordId (u64).
///
/// This directory maintains a bidirectional mapping so that:
/// - The query layer works with logical IDs (collection:key strings)
/// - The storage kernel works with physical IDs (compact u64)
/// - The mapping is stable across restarts (when serialized)
#[derive(Debug, Default)]
pub struct RecordDirectory {
    /// Forward: "collection:key" -> physical u64
    forward: BTreeMap<String, u64>,
    /// Reverse: physical u64 -> "collection:key"
    reverse: BTreeMap<u64, String>,
    /// Next physical RID to assign
    next_rid: u64,
}

impl RecordDirectory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a physical RID for a logical RecordId.
    pub fn resolve_or_create(&mut self, logical: &RecordId) -> crate::store::RecordId {
        let key = logical.storage_key();
        if let Some(&physical) = self.forward.get(&key) {
            return crate::store::RecordId(physical);
        }
        let physical = self.next_rid;
        self.next_rid += 1;
        self.forward.insert(key.clone(), physical);
        self.reverse.insert(physical, key);
        crate::store::RecordId(physical)
    }

    /// Look up the physical RID for a logical RecordId (returns None if not mapped).
    pub fn resolve(&self, logical: &RecordId) -> Option<crate::store::RecordId> {
        self.forward
            .get(&logical.storage_key())
            .map(|&physical| crate::store::RecordId(physical))
    }

    /// Look up the logical RecordId for a physical RID.
    pub fn lookup(&self, physical: crate::store::RecordId) -> Option<RecordId> {
        self.reverse.get(&physical.0).map(|key| {
            let parts: Vec<&str> = key.splitn(2, ':').collect();
            RecordId {
                collection: parts.first().unwrap_or(&"").to_string(),
                key: parts.get(1).unwrap_or(&"").to_string(),
            }
        })
    }

    /// Check if a logical RecordId is mapped.
    pub fn contains(&self, logical: &RecordId) -> bool {
        self.forward.contains_key(&logical.storage_key())
    }

    pub fn len(&self) -> usize {
        self.forward.len()
    }

    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }
}
