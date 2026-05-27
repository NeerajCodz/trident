use criterion::{Criterion, black_box, criterion_group, criterion_main};
use praxis::formats::codecs::FormatCodec;
use praxis::formats::codecs::row::{RowRecord, RowRecordCodec};

fn bench_row_codec(c: &mut Criterion) {
    let row = RowRecord {
        record_id: 42,
        bytes: vec![0xaa; 4096],
    };

    c.bench_function("row_record_encode", |b| {
        b.iter(|| black_box(RowRecordCodec::encode(black_box(&row)).unwrap()))
    });
}

criterion_group!(benches, bench_row_codec);
criterion_main!(benches);
