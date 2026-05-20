//! Query planner: converts logical plans to physical plans with cost-based optimization.

pub mod plan;

pub use plan::{
    AggregateExpr, CostedOperator, CostedPlan, PhysicalOperator, PhysicalPlan, Planner,
    PlannerStats,
};
