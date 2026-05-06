use criterion::{Criterion, black_box, criterion_group, criterion_main};
use trident::{BoundingBox, PackedRTreeIndex, RecordId};

fn bench_spatial_intersection(c: &mut Criterion) {
    let mut index = PackedRTreeIndex::default();
    for i in 0..10_000_u64 {
        let x = i as f64;
        index.insert(
            BoundingBox {
                min_x: x,
                min_y: x,
                max_x: x + 1.0,
                max_y: x + 1.0,
            },
            RecordId(i),
        );
    }

    let query = BoundingBox {
        min_x: 100.0,
        min_y: 100.0,
        max_x: 200.0,
        max_y: 200.0,
    };
    c.bench_function("spatial_intersection", |b| {
        b.iter(|| black_box(index.search(black_box(query))))
    });
}

criterion_group!(benches, bench_spatial_intersection);
criterion_main!(benches);
