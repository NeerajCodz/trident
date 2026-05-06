use criterion::{black_box, criterion_group, criterion_main, Criterion};
use trident::query::hybrid::HybridPlan;
use trident::query::QueryModel;

fn bench_hybrid_planning(c: &mut Criterion) {
    c.bench_function("hybrid_sql_vector_graph_search_plan", |b| {
        b.iter(|| {
            black_box(HybridPlan::from_models(&[
                QueryModel::Sql,
                QueryModel::Vector,
                QueryModel::Graph,
                QueryModel::Search,
            ]))
        })
    });
}

criterion_group!(benches, bench_hybrid_planning);
criterion_main!(benches);
