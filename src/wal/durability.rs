#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurabilitySource {
    Wal,
    Checkpoint,
    Snapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalVisibilityRule {
    DurableBeforeVisible,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalDurability {
    pub source: DurabilitySource,
    pub visibility_rule: WalVisibilityRule,
    pub sequence: u64,
    pub synced: bool,
}

impl WalDurability {
    pub fn durable_wal(sequence: u64) -> Self {
        Self {
            source: DurabilitySource::Wal,
            visibility_rule: WalVisibilityRule::DurableBeforeVisible,
            sequence,
            synced: true,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.synced && self.visibility_rule == WalVisibilityRule::DurableBeforeVisible
    }
}
