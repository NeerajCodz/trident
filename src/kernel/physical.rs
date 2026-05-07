#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalEngineKind {
    ValueStore,
    RecordDirectory,
    Lsm,
    BTree,
    Art,
    Hash,
    Bitmap,
    Inverted,
    Hnsw,
    Ivf,
    GraphAdjacency,
    ColumnarProjection,
    WideColumn,
    TimeSeries,
    LogStream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineRole {
    CanonicalValueOwner,
    PointerIndex,
    DerivedMaterializedLayout,
    LossySummary,
    ReplicationPrimitive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineCapability {
    PointLookup,
    RangeScan,
    PrefixScan,
    OrderedIteration,
    FilteredScan,
    VectorSearch,
    GraphTraversal,
    FullTextSearch,
    ColumnarScan,
    AppendOnlyLog,
    SnapshotTransfer,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalEngine {
    pub name: String,
    pub kind: PhysicalEngineKind,
    pub role: EngineRole,
    pub capabilities: Vec<EngineCapability>,
    pub stores_canonical_values: bool,
    pub pointer_oriented: bool,
}

impl PhysicalEngine {
    pub fn pointer_index(
        name: impl Into<String>,
        kind: PhysicalEngineKind,
        capabilities: Vec<EngineCapability>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            role: EngineRole::PointerIndex,
            capabilities,
            stores_canonical_values: false,
            pointer_oriented: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueStore {
    pub name: String,
    pub canonical_owner: bool,
}

impl Default for ValueStore {
    fn default() -> Self {
        Self {
            name: "value_store".to_string(),
            canonical_owner: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedLayout {
    pub name: String,
    pub source_engine: String,
    pub canonical: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalyticalProjection {
    pub name: String,
    pub source_engine: String,
    pub derived: bool,
}
