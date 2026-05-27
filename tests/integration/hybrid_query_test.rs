use praxis::kernel::ExecutionMode;
use praxis::query::QueryModel;
use praxis::query::hybrid::{HybridPlan, HybridQuery};

#[test]
fn hybrid_plan_orders_specialized_steps_by_cost() {
    let plan = HybridPlan::from_models(&[
        QueryModel::Vector,
        QueryModel::Search,
        QueryModel::Sql,
        QueryModel::Graph,
    ]);

    assert_eq!(plan.execution_mode, ExecutionMode::Hybrid);
    assert_eq!(
        plan.indexes(),
        vec!["inverted", "btree", "adjacency", "hnsw_or_ivf"]
    );
    assert_eq!(
        plan.cursors(),
        vec!["postings", "relational_scan", "graph_traversal", "ann_hits"]
    );
    assert_eq!(plan.final_cursor, "hybrid_bitmap_join");
    assert!(plan.estimated_cost_units > 0);
}

#[test]
fn hybrid_query_deduplicates_models_and_ignores_nested_hybrid_marker() {
    let query = HybridQuery::new(vec![
        QueryModel::Hybrid,
        QueryModel::Search,
        QueryModel::Search,
        QueryModel::Vector,
    ])
    .with_predicate(b"tenant=a".to_vec())
    .with_limit(10);

    let plan = query.plan();

    assert_eq!(query.limit, 10);
    assert_eq!(query.predicate, Some(b"tenant=a".to_vec()));
    assert_eq!(plan.indexes(), vec!["inverted", "hnsw_or_ivf"]);
}
