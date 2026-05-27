use praxis::RecordId;
use praxis::index::adjacency::AdjacencyIndex;
use praxis::kernel::ExecutionMode;
use praxis::query::graph::{GraphTraversal, traverse};
use praxis::query::{QueryModel, plan};

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

#[test]
fn graph_traversal_cursor_walks_breadth_first_with_label_filter() {
    let dir = tempfile::tempdir().unwrap();
    let mut index = AdjacencyIndex::open("graph", dir.path()).unwrap();
    index
        .add_edge(
            RecordId(1),
            b"knows",
            RecordId(2),
            std::collections::BTreeMap::new(),
        )
        .unwrap();
    index
        .add_edge(
            RecordId(1),
            b"likes",
            RecordId(9),
            std::collections::BTreeMap::new(),
        )
        .unwrap();
    index
        .add_edge(
            RecordId(2),
            b"knows",
            RecordId(3),
            std::collections::BTreeMap::new(),
        )
        .unwrap();
    index
        .add_edge(
            RecordId(3),
            b"knows",
            RecordId(1),
            std::collections::BTreeMap::new(),
        )
        .unwrap();

    let hits = traverse(
        &index,
        &GraphTraversal {
            start: RecordId(1),
            max_depth: 2,
            edge_label: Some("knows".into()),
        },
    );

    assert_eq!(
        hits,
        vec![
            praxis::query::graph::GraphTraversalHit {
                record_id: RecordId(1),
                depth: 0
            },
            praxis::query::graph::GraphTraversalHit {
                record_id: RecordId(2),
                depth: 1
            },
            praxis::query::graph::GraphTraversalHit {
                record_id: RecordId(3),
                depth: 2
            }
        ]
    );
}

#[test]
fn graph_traversal_reflects_deleted_edges() {
    let dir = tempfile::tempdir().unwrap();
    let mut index = AdjacencyIndex::open("graph", dir.path()).unwrap();
    index
        .add_edge(
            RecordId(1),
            b"knows",
            RecordId(2),
            std::collections::BTreeMap::new(),
        )
        .unwrap();
    index.remove_edges(RecordId(1), RecordId(2));

    let hits = traverse(
        &index,
        &GraphTraversal {
            start: RecordId(1),
            max_depth: 1,
            edge_label: None,
        },
    );

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].record_id, RecordId(1));
}
