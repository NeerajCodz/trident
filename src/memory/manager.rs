use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum MemoryPoolKind {
    Arena,
    Slab,
    Epoch,
    RefCount,
    Page,
    VectorAligned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlabClass {
    pub object_size: usize,
    pub capacity: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NumaPartition {
    pub node: u16,
    pub memory_budget_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SimdAlignment {
    pub bytes: usize,
}

impl Default for SimdAlignment {
    fn default() -> Self {
        Self { bytes: 64 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpillPolicy {
    pub threshold_bytes: usize,
    pub spill_to_disk: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdaptiveMemoryQuota {
    pub hard_limit_bytes: usize,
    pub soft_limit_bytes: usize,
}

#[derive(Clone, Debug, Default)]
pub struct MemoryManager {
    quotas: BTreeMap<MemoryPoolKind, AdaptiveMemoryQuota>,
    used: BTreeMap<MemoryPoolKind, usize>,
}

impl MemoryManager {
    pub fn set_quota(&mut self, pool: MemoryPoolKind, quota: AdaptiveMemoryQuota) {
        self.quotas.insert(pool, quota);
    }

    pub fn reserve(&mut self, pool: MemoryPoolKind, bytes: usize) -> bool {
        let used = self.used.get(&pool).copied().unwrap_or(0);
        let hard_limit = self
            .quotas
            .get(&pool)
            .map(|quota| quota.hard_limit_bytes)
            .unwrap_or(usize::MAX);
        if used.saturating_add(bytes) > hard_limit {
            return false;
        }
        self.used.insert(pool, used + bytes);
        true
    }

    pub fn release(&mut self, pool: MemoryPoolKind, bytes: usize) {
        let used = self.used.get(&pool).copied().unwrap_or(0);
        self.used.insert(pool, used.saturating_sub(bytes));
    }

    pub fn used_bytes(&self, pool: MemoryPoolKind) -> usize {
        self.used.get(&pool).copied().unwrap_or(0)
    }

    pub fn should_spill(&self, pool: MemoryPoolKind, policy: SpillPolicy) -> bool {
        policy.spill_to_disk && self.used_bytes(pool) >= policy.threshold_bytes
    }
}
