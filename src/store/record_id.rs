use serde::{Deserialize, Serialize};

/// Stable logical identifier for a record in the primary data store.
///
/// A `RecordId` is an opaque 64-bit integer.  It is stable across
/// compaction because the [`IndirectionTable`][super::IndirectionTable]
/// maps each logical id to a physical `(segment_id, record_offset,
/// length)`.  All index plugins store `key → RecordId` and never
/// duplicate the underlying value bytes.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
)]
pub struct RecordId(pub u64);

impl RecordId {
    /// The null / sentinel value.  Never returned by a live write.
    pub const NULL: Self = Self(0);

    pub fn is_null(self) -> bool {
        self.0 == 0
    }
}

impl std::fmt::Display for RecordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RID({})", self.0)
    }
}
