use trident::kernel::ExecutionMode;
use trident::query::sql::{SqlIndexKind, SqlIndexSpec};
use trident::query::{plan, QueryModel};

#[test]
fn sql_plans_to_btree_specialized_path() {
    let query_plan = plan(QueryModel::Sql);
    assert_eq!(query_plan.execution_mode, ExecutionMode::Specialized);
    assert_eq!(query_plan.index, "btree");
    assert_eq!(query_plan.cursor, "relational_scan");
    assert!(query_plan.cost_units > 0);
}

#[test]
fn sql_index_spec_captures_unique_covering_indexes() {
    let spec = SqlIndexSpec {
        table: "users".into(),
        columns: vec!["email".into()],
        unique: true,
        covering: vec!["name".into()],
    };
    assert!(spec.unique);
    assert_eq!(spec.columns, ["email"]);
    assert_eq!(spec.kind(), SqlIndexKind::UniqueBTree);
}

#[test]
fn sql_index_spec_builds_composite_keys_and_costs() {
    let spec = SqlIndexSpec {
        table: "events".into(),
        columns: vec!["tenant_id".into(), "created_at".into()],
        unique: false,
        covering: vec!["payload".into()],
    };

    let key = spec
        .composite_key(&[b"tenant-a".as_slice(), b"2026-05-06".as_slice()])
        .unwrap();
    let cost = spec.cost_metadata(1_000_000, 10_000);

    assert!(spec.is_composite());
    assert_eq!(spec.kind(), SqlIndexKind::CompositeBTree);
    assert!(key.starts_with(&8_u32.to_be_bytes()));
    assert!(cost.range_scan_cost >= cost.point_lookup_cost);
}
