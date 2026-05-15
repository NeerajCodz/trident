use trident::api::graphql::{GraphQlPrimitive, STORAGE_GRAPHQL_SCHEMA};

#[test]
fn graphql_schema_exposes_storage_primitives() {
    assert!(STORAGE_GRAPHQL_SCHEMA.contains("putRecord"));
    assert!(STORAGE_GRAPHQL_SCHEMA.contains("putRecordBatch"));
    assert!(STORAGE_GRAPHQL_SCHEMA.contains("BatchRecordInput"));
    assert!(STORAGE_GRAPHQL_SCHEMA.contains("deleteRecord"));
    assert!(STORAGE_GRAPHQL_SCHEMA.contains("recordId"));
    assert!(STORAGE_GRAPHQL_SCHEMA.contains("RequestContextInput"));
    assert_eq!(GraphQlPrimitive::PutRecord.field_name(), "putRecord");
    assert_eq!(
        GraphQlPrimitive::PutRecordBatch.field_name(),
        "putRecordBatch"
    );
}
