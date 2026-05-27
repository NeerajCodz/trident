use criterion::{Criterion, black_box, criterion_group, criterion_main};
use praxis::index::IndexPlugin;
use praxis::{BTreeIndex, RecordId};
use tempfile::tempdir;

fn bench_btree_secondary_index(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let mut index = BTreeIndex::open("sql_secondary", dir.path()).unwrap();
    for i in 0..10_000 {
        index
            .put(format!("user:{i:08}").as_bytes(), RecordId(i))
            .unwrap();
    }

    c.bench_function("btree_secondary_lookup", |b| {
        b.iter(|| black_box(index.get(black_box(b"user:00005000"))))
    });
}

criterion_group!(benches, bench_btree_secondary_index);
criterion_main!(benches);
