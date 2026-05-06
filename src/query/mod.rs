use crate::kernel::ExecutionMode;

pub mod graph;
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
}

pub fn plan(model: QueryModel) -> QueryPlan {
    match model {
        QueryModel::Kv => QueryPlan {
            model,
            execution_mode: ExecutionMode::Specialized,
            index: "lsm",
        },
        QueryModel::Sql => QueryPlan {
            model,
            execution_mode: ExecutionMode::Specialized,
            index: "btree",
        },
        QueryModel::Graph => QueryPlan {
            model,
            execution_mode: ExecutionMode::Specialized,
            index: "adjacency",
        },
        QueryModel::Vector => QueryPlan {
            model,
            execution_mode: ExecutionMode::Specialized,
            index: "hnsw_or_ivf",
        },
        QueryModel::Search => QueryPlan {
            model,
            execution_mode: ExecutionMode::Specialized,
            index: "inverted",
        },
        QueryModel::Hybrid => QueryPlan {
            model,
            execution_mode: ExecutionMode::Hybrid,
            index: "multi_index",
        },
    }
}
