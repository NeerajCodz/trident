pub const STORAGE_PROTO: &str = include_str!("../../../proto/trident/storage.proto");
pub const QUERY_PROTO: &str = include_str!("../../../proto/trident/query.proto");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrpcService {
    StorageAdmin,
    Kv,
    Sql,
    Graph,
    Vector,
    Search,
    Benchmark,
}
