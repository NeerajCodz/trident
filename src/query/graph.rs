use crate::store::RecordId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphTraversal {
    pub start: RecordId,
    pub max_depth: u8,
    pub edge_label: Option<String>,
}
