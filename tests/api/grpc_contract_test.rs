use trident::api::grpc::{GrpcService, QUERY_PROTO, STORAGE_PROTO};

#[test]
fn grpc_proto_contracts_are_embedded() {
    assert!(STORAGE_PROTO.contains("service StorageAdmin"));
    assert!(STORAGE_PROTO.contains("service Kv"));
    assert!(STORAGE_PROTO.contains("message RequestContext"));
    assert!(STORAGE_PROTO.contains("request_id"));
    assert!(QUERY_PROTO.contains("service Vector"));
    assert!(QUERY_PROTO.contains("message RequestContext"));
    assert_eq!(GrpcService::Search, GrpcService::Search);
}
