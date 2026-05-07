use crate::config::Compression;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum StorageTier {
    Hot,
    Warm,
    Cold,
    Frozen,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectStoreLocator {
    pub bucket: String,
    pub key: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompressionEscalation {
    pub warm: Compression,
    pub cold: Compression,
    pub frozen: Compression,
}

impl Default for CompressionEscalation {
    fn default() -> Self {
        Self {
            warm: Compression::Lz4,
            cold: Compression::Zstd,
            frozen: Compression::Zstd,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TierPlacement {
    pub tier: StorageTier,
    pub compression: Compression,
    pub object_locator: Option<ObjectStoreLocator>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TieredStoragePolicy {
    pub hot_threshold: u64,
    pub warm_threshold: u64,
    pub escalation: CompressionEscalation,
}

impl Default for TieredStoragePolicy {
    fn default() -> Self {
        Self {
            hot_threshold: 1_000,
            warm_threshold: 100,
            escalation: CompressionEscalation::default(),
        }
    }
}

impl TieredStoragePolicy {
    pub fn heat_score(reads: u64, writes: u64, age_seconds: u64) -> u64 {
        reads
            .saturating_add(writes.saturating_mul(4))
            .saturating_mul(1_000)
            / age_seconds.max(1)
    }

    pub fn place(&self, heat_score: u64) -> TierPlacement {
        if heat_score >= self.hot_threshold {
            TierPlacement {
                tier: StorageTier::Hot,
                compression: Compression::None,
                object_locator: None,
            }
        } else if heat_score >= self.warm_threshold {
            TierPlacement {
                tier: StorageTier::Warm,
                compression: self.escalation.warm,
                object_locator: None,
            }
        } else {
            TierPlacement {
                tier: StorageTier::Cold,
                compression: self.escalation.cold,
                object_locator: None,
            }
        }
    }

    pub fn freeze(&self, locator: ObjectStoreLocator) -> TierPlacement {
        TierPlacement {
            tier: StorageTier::Frozen,
            compression: self.escalation.frozen,
            object_locator: Some(locator),
        }
    }
}
