use trident::RecordId;
use trident::kernel::ExecutionMode;
use trident::query::graph::GraphTraversal;
use trident::query::{QueryModel, plan};

#[test]
fn graph_plans_to_adjacency_path() {
    let query_plan = plan(QueryModel::Graph);
    assert_eq!(query_plan.execution_mode, ExecutionMode::Specialized);
    assert_eq!(query_plan.index, "adjacency");
}

#[test]
fn graph_traversal_request_is_stable() {
    let traversal = GraphTraversal {
        start: RecordId(7),
        max_depth: 3,
        edge_label: Some("knows".into()),
    };
    assert_eq!(traversal.start, RecordId(7));
    assert_eq!(traversal.max_depth, 3);
}
