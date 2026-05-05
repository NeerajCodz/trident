use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct GcReport {
    pub files_reclaimed: u64,
    pub bytes_reclaimed: u64,
}
