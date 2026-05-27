use criterion::{Criterion, black_box, criterion_group, criterion_main};
use praxis::{IvfFlatIndex, RecordId};

fn bench_ivf_exact_search(c: &mut Criterion) {
    let mut index = IvfFlatIndex::new(4, 16);
    for i in 0..10_000_u64 {
        let base = i as f32;
        index.insert(RecordId(i), vec![base, base + 1.0, base + 2.0, base + 3.0]);
    }

    c.bench_function("ivf_exact_search", |b| {
        b.iter(|| black_box(index.search_exact(black_box(&[50.0, 51.0, 52.0, 53.0]), 10)))
    });
}

criterion_group!(benches, bench_ivf_exact_search);
criterion_main!(benches);
