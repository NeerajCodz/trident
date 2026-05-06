use criterion::{Criterion, black_box, criterion_group, criterion_main};
use tempfile::tempdir;
use trident::RecordId;
use trident::index::hnsw::adjacency::AdjacencyIndex;

fn bench_adjacency_one_hop(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let mut index = AdjacencyIndex::open("graph", dir.path()).unwrap();
    for i in 0..10_000_u64 {
        index
            .add_edge(RecordId(i), b"next", RecordId(i + 1))
            .unwrap();
    }

    c.bench_function("adjacency_one_hop", |b| {
        b.iter(|| black_box(index.neighbors(black_box(RecordId(5_000)))))
    });
}

criterion_group!(benches, bench_adjacency_one_hop);
criterion_main!(benches);
