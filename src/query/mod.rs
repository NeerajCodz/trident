use crate::kernel::ExecutionMode;

pub mod graph;
pub mod hybrid;
pub mod kv;
pub mod search;
pub mod sql;
pub mod vector;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryModel {
    Kv,
    Sql,
    Graph,
    Vector,
    Search,
    Hybrid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryPlan {
    pub model: QueryModel,
    pub execution_mode: ExecutionMode,
    pub index: &'static str,
    pub cursor: &'static str,
    pub cost_units: u64,
}

pub fn plan(model: QueryModel) -> QueryPlan {
    match model {
        QueryModel::Kv => QueryPlan {
            model,
            execution_mode: ExecutionMode::Specialized,
            index: "lsm",
            cursor: "kv_point_or_range",
            cost_units: 10,
        },
        QueryModel::Sql => QueryPlan {
            model,
            execution_mode: ExecutionMode::Specialized,
            index: "btree",
            cursor: "relational_scan",
            cost_units: 35,
        },
        QueryModel::Graph => QueryPlan {
            model,
            execution_mode: ExecutionMode::Specialized,
            index: "adjacency",
            cursor: "graph_traversal",
            cost_units: 45,
        },
        QueryModel::Vector => QueryPlan {
            model,
            execution_mode: ExecutionMode::Specialized,
            index: "hnsw_or_ivf",
            cursor: "ann_hits",
            cost_units: 55,
        },
        QueryModel::Search => QueryPlan {
            model,
            execution_mode: ExecutionMode::Specialized,
            index: "inverted",
            cursor: "postings",
            cost_units: 30,
        },
        QueryModel::Hybrid => QueryPlan {
            model,
            execution_mode: ExecutionMode::Hybrid,
            index: "multi_index",
            cursor: "hybrid_bitmap_join",
            cost_units: 100,
        },
    }
}
