#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlQuery {
    pub statement: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqlIndexKind {
    BTree,
    UniqueBTree,
    CompositeBTree,
    CoveringBTree,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlCostMetadata {
    pub estimated_rows: u64,
    pub distinct_keys: u64,
    pub covering_columns: u64,
    pub point_lookup_cost: u64,
    pub range_scan_cost: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlIndexSpec {
    pub table: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub covering: Vec<String>,
}

impl SqlIndexSpec {
    pub fn kind(&self) -> SqlIndexKind {
        if self.unique {
            SqlIndexKind::UniqueBTree
        } else if self.columns.len() > 1 {
            SqlIndexKind::CompositeBTree
        } else if !self.covering.is_empty() {
            SqlIndexKind::CoveringBTree
        } else {
            SqlIndexKind::BTree
        }
    }

    pub fn is_composite(&self) -> bool {
        self.columns.len() > 1
    }

    pub fn composite_key(&self, values: &[&[u8]]) -> Option<Vec<u8>> {
        if values.len() != self.columns.len() {
            return None;
        }
        let mut out = Vec::new();
        for value in values {
            out.extend_from_slice(&(value.len() as u32).to_be_bytes());
            out.extend_from_slice(value);
        }
        Some(out)
    }

    pub fn cost_metadata(&self, estimated_rows: u64, distinct_keys: u64) -> SqlCostMetadata {
        let covering_columns = self.covering.len() as u64;
        let fanout = estimated_rows.saturating_div(distinct_keys.max(1)).max(1);
        SqlCostMetadata {
            estimated_rows,
            distinct_keys,
            covering_columns,
            point_lookup_cost: fanout,
            range_scan_cost: fanout
                .saturating_mul(self.columns.len().max(1) as u64)
                .saturating_sub(covering_columns.min(fanout)),
        }
    }
}
