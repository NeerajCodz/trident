use crate::kernel::ExecutionMode;
use crate::query::{QueryModel, QueryPlan, plan};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HybridQuery {
    pub models: Vec<QueryModel>,
    pub predicate: Option<Vec<u8>>,
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HybridStep {
    pub model: QueryModel,
    pub index: &'static str,
    pub cursor: &'static str,
    pub cost_units: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HybridPlan {
    pub execution_mode: ExecutionMode,
    pub steps: Vec<HybridStep>,
    pub final_cursor: &'static str,
    pub estimated_cost_units: u64,
}

impl HybridQuery {
    pub fn new(models: Vec<QueryModel>) -> Self {
        Self {
            models,
            predicate: None,
            limit: 100,
        }
    }

    pub fn with_predicate(mut self, predicate: Vec<u8>) -> Self {
        self.predicate = Some(predicate);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn plan(&self) -> HybridPlan {
        HybridPlan::from_models(&self.models)
    }
}

impl HybridPlan {
    pub fn from_models(models: &[QueryModel]) -> Self {
        let mut steps: Vec<_> = models
            .iter()
            .copied()
            .filter(|model| *model != QueryModel::Hybrid)
            .map(plan)
            .map(HybridStep::from)
            .collect();

        steps.sort_by_key(|step| (step.cost_units, model_rank(step.model)));
        steps.dedup_by_key(|step| step.model);

        let estimated_cost_units = steps.iter().map(|step| step.cost_units).sum();
        Self {
            execution_mode: ExecutionMode::Hybrid,
            steps,
            final_cursor: "hybrid_bitmap_join",
            estimated_cost_units,
        }
    }

    pub fn indexes(&self) -> Vec<&'static str> {
        self.steps.iter().map(|step| step.index).collect()
    }

    pub fn cursors(&self) -> Vec<&'static str> {
        self.steps.iter().map(|step| step.cursor).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

impl From<QueryPlan> for HybridStep {
    fn from(plan: QueryPlan) -> Self {
        Self {
            model: plan.model,
            index: plan.index,
            cursor: plan.cursor,
            cost_units: plan.cost_units,
        }
    }
}

fn model_rank(model: QueryModel) -> u8 {
    match model {
        QueryModel::Kv => 0,
        QueryModel::Search => 1,
        QueryModel::Sql => 2,
        QueryModel::Graph => 3,
        QueryModel::Vector => 4,
        QueryModel::Hybrid => 5,
    }
}
