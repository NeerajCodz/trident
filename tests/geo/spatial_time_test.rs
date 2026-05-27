use praxis::{BoundingBox, PackedRTreeIndex, RecordId, TimePartition};

#[test]
fn spatial_index_filters_by_intersection() {
    let mut index = PackedRTreeIndex::default();
    index.insert(
        BoundingBox {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 10.0,
            max_y: 10.0,
        },
        RecordId(1),
    );

    let hits = index.search(BoundingBox {
        min_x: 5.0,
        min_y: 5.0,
        max_x: 6.0,
        max_y: 6.0,
    });
    assert_eq!(hits, vec![RecordId(1)]);
}

#[test]
fn time_partition_filters_by_range() {
    let mut partition = TimePartition::new(100, 200);
    assert!(partition.insert(150, RecordId(1)));
    assert!(!partition.insert(250, RecordId(2)));
    assert_eq!(partition.range(120, 170), vec![RecordId(1)]);
}
