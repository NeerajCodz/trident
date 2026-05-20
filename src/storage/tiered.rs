use crate::config::Compression;
use crate::store::RecordId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum StorageTier {
    Hot,
    Warm,
    Cold,
    Frozen,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObjectStoreLocator {
    pub bucket: String,
    pub key: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TierPlacement {
    pub tier: StorageTier,
    pub compression: Compression,
    pub object_locator: Option<ObjectStoreLocator>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TieredStoragePolicy {
    pub hot_threshold: u64,
    pub warm_threshold: u64,
    pub escalation: CompressionEscalation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TierHeatSample {
    pub reads: u64,
    pub writes: u64,
    pub age_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TierMigrationRequest {
    pub record_id: RecordId,
    pub current_tier: StorageTier,
    pub heat: TierHeatSample,
    pub object_locator: Option<ObjectStoreLocator>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TierMigrationStatus {
    Planned,
    Started,
    Installed,
    Aborted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TierMigrationRecord {
    pub migration_id: u64,
    pub record_id: RecordId,
    pub from_tier: StorageTier,
    pub to: TierPlacement,
    pub status: TierMigrationStatus,
    pub manifest_edit_started: bool,
    pub manifest_edit_installed: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TierMigrationManifest {
    pub next_migration_id: u64,
    pub records: Vec<TierMigrationRecord>,
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

    pub fn plan_migration(&self, request: TierMigrationRequest) -> Option<TierPlacement> {
        let heat_score = Self::heat_score(
            request.heat.reads,
            request.heat.writes,
            request.heat.age_seconds,
        );
        let target = match request.object_locator {
            Some(locator)
                if heat_score < self.warm_threshold
                    && request.current_tier == StorageTier::Cold =>
            {
                self.freeze(locator)
            }
            _ => self.place(heat_score),
        };

        (target.tier != request.current_tier).then_some(target)
    }
}

impl TierMigrationManifest {
    pub fn plan(
        &mut self,
        request: TierMigrationRequest,
        policy: &TieredStoragePolicy,
    ) -> Option<TierMigrationRecord> {
        let to = policy.plan_migration(request.clone())?;
        let migration_id = self.next_migration_id.max(1);
        self.next_migration_id = migration_id.saturating_add(1);
        let record = TierMigrationRecord {
            migration_id,
            record_id: request.record_id,
            from_tier: request.current_tier,
            to,
            status: TierMigrationStatus::Planned,
            manifest_edit_started: false,
            manifest_edit_installed: false,
        };
        self.records.push(record.clone());
        Some(record)
    }

    pub fn mark_started(&mut self, migration_id: u64) -> Option<&TierMigrationRecord> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.migration_id == migration_id)?;
        record.status = TierMigrationStatus::Started;
        record.manifest_edit_started = true;
        Some(record)
    }

    pub fn mark_installed(&mut self, migration_id: u64) -> Option<&TierMigrationRecord> {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.migration_id == migration_id)?;
        record.status = TierMigrationStatus::Installed;
        record.manifest_edit_installed = true;
        Some(record)
    }

    pub fn recover_incomplete(&mut self) -> Vec<TierMigrationRecord> {
        let mut cleanup = Vec::new();
        for record in &mut self.records {
            if record.status == TierMigrationStatus::Started && !record.manifest_edit_installed {
                record.status = TierMigrationStatus::Aborted;
                cleanup.push(record.clone());
            }
        }
        cleanup
    }
}

/// Tier migration engine that automatically moves data between tiers.
pub struct TierMigrationEngine {
    policy: TieredStoragePolicy,
    manifest: TierMigrationManifest,
    heat_samples: std::collections::BTreeMap<u64, TierHeatSample>,
    /// Maximum migrations per cycle
    max_migrations_per_cycle: usize,
}

impl TierMigrationEngine {
    pub fn new(policy: TieredStoragePolicy) -> Self {
        Self {
            policy,
            manifest: TierMigrationManifest::default(),
            heat_samples: std::collections::BTreeMap::new(),
            max_migrations_per_cycle: 10,
        }
    }

    /// Record a heat sample for a record.
    pub fn record_access(&mut self, record_id: u64, is_write: bool) {
        let sample = self.heat_samples.entry(record_id).or_insert(TierHeatSample {
            reads: 0,
            writes: 0,
            age_seconds: 0,
        });
        if is_write {
            sample.writes += 1;
        } else {
            sample.reads += 1;
        }
    }

    /// Update age for all tracked records.
    pub fn tick(&mut self, elapsed_seconds: u64) {
        for sample in self.heat_samples.values_mut() {
            sample.age_seconds += elapsed_seconds;
        }
    }

    /// Run a migration cycle. Returns planned migrations.
    pub fn run_cycle(&mut self, current_tier: StorageTier) -> Vec<TierMigrationRecord> {
        let mut migrations = Vec::new();

        for (record_id, heat) in &self.heat_samples {
            if migrations.len() >= self.max_migrations_per_cycle {
                break;
            }

            let request = TierMigrationRequest {
                record_id: RecordId(*record_id),
                current_tier,
                heat: heat.clone(),
                object_locator: None,
            };

            if let Some(record) = self.manifest.plan(request, &self.policy) {
                migrations.push(record);
            }
        }

        migrations
    }

    /// Get the recommended tier for a record based on its heat.
    pub fn recommended_tier(&self, record_id: u64) -> StorageTier {
        let heat = self.heat_samples.get(&record_id);
        let score = heat
            .map(|h| TieredStoragePolicy::heat_score(h.reads, h.writes, h.age_seconds))
            .unwrap_or(0);
        self.policy.place(score).tier
    }

    /// Get migration manifest.
    pub fn manifest(&self) -> &TierMigrationManifest {
        &self.manifest
    }

    /// Get mutable migration manifest.
    pub fn manifest_mut(&mut self) -> &mut TierMigrationManifest {
        &mut self.manifest
    }

    /// Get heat samples.
    pub fn heat_samples(&self) -> &std::collections::BTreeMap<u64, TierHeatSample> {
        &self.heat_samples
    }

    /// Clear old heat samples.
    pub fn prune_samples(&mut self, max_age_seconds: u64) {
        self.heat_samples.retain(|_, sample| sample.age_seconds < max_age_seconds);
    }
}

impl Default for TierMigrationEngine {
    fn default() -> Self {
        Self::new(TieredStoragePolicy::default())
    }
}
