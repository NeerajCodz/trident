use criterion::{Criterion, black_box, criterion_group, criterion_main};
use praxis::{InvertedIndex, RecordId};

fn bench_inverted_search(c: &mut Criterion) {
    let mut index = InvertedIndex::new("search");
    for i in 0..10_000_u64 {
        index.index_text("fast storage vector graph search", RecordId(i));
    }

    c.bench_function("inverted_term_search", |b| {
        b.iter(|| black_box(index.search(black_box("vector"))))
    });
}

criterion_group!(benches, bench_inverted_search);
criterion_main!(benches);
