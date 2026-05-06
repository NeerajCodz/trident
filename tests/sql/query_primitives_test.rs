use trident::kernel::ExecutionMode;
use trident::query::sql::SqlIndexSpec;
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
}
