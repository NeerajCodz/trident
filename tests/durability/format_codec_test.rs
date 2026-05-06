use trident::formats::codecs::FormatCodec;
use trident::formats::codecs::bitmap::{BitmapBlock, BitmapCodec};
use trident::formats::codecs::checkpoint::{CheckpointBlock, CheckpointCodec};
use trident::formats::codecs::columnar::{ColumnarBlock, ColumnarBlockCodec};
use trident::formats::codecs::filter::{FilterBlock, FilterCodec};
use trident::formats::codecs::postings::{PostingsCodec, PostingsList};
use trident::formats::codecs::row::{RowRecord, RowRecordCodec};
use trident::formats::codecs::vector_graph::{VectorGraphCodec, VectorGraphNode};

#[test]
fn versioned_format_codecs_roundtrip() {
    let row = RowRecord {
        record_id: 7,
        bytes: b"row".to_vec(),
    };
    assert_eq!(
        RowRecordCodec::decode(&RowRecordCodec::encode(&row).unwrap()).unwrap(),
        row
    );

    let columnar = ColumnarBlock {
        column_id: 2,
        values: vec![b"a".to_vec(), b"bb".to_vec()],
    };
    assert_eq!(
        ColumnarBlockCodec::decode(&ColumnarBlockCodec::encode(&columnar).unwrap()).unwrap(),
        columnar
    );

    let postings = PostingsList {
        term: b"trident".to_vec(),
        record_ids: vec![1, 2, 3],
    };
    assert_eq!(
        PostingsCodec::decode(&PostingsCodec::encode(&postings).unwrap()).unwrap(),
        postings
    );

    let bitmap = BitmapBlock {
        bits: vec![1, 2, 4],
    };
    assert_eq!(
        BitmapCodec::decode(&BitmapCodec::encode(&bitmap).unwrap()).unwrap(),
        bitmap
    );

    let filter = FilterBlock {
        min_key: b"a".to_vec(),
        max_key: b"z".to_vec(),
        bloom_bits: vec![0xff],
    };
    assert_eq!(
        FilterCodec::decode(&FilterCodec::encode(&filter).unwrap()).unwrap(),
        filter
    );

    let checkpoint = CheckpointBlock {
        sequence: 10,
        manifest_epoch: 11,
    };
    assert_eq!(
        CheckpointCodec::decode(&CheckpointCodec::encode(&checkpoint).unwrap()).unwrap(),
        checkpoint
    );

    let vector = VectorGraphNode {
        record_id: 42,
        vector: vec![1.0, 2.0],
        neighbors: vec![9, 10],
    };
    assert_eq!(
        VectorGraphCodec::decode(&VectorGraphCodec::encode(&vector).unwrap()).unwrap(),
        vector
    );
}

#[test]
fn format_version_mismatch_is_rejected() {
    let row = RowRecord {
        record_id: 1,
        bytes: b"x".to_vec(),
    };
    let mut encoded = RowRecordCodec::encode(&row).unwrap();
    encoded[6] = 2;
    assert!(RowRecordCodec::decode(&encoded).is_err());
}

#[test]
fn malformed_format_payloads_return_errors() {
    for bytes in [
        b"".as_slice(),
        b"TFMT".as_slice(),
        b"TFMT\x01\x00\x01".as_slice(),
    ] {
        assert!(RowRecordCodec::decode(bytes).is_err());
    }

    let mut encoded = ColumnarBlockCodec::encode(&ColumnarBlock {
        column_id: 1,
        values: vec![b"abc".to_vec()],
    })
    .unwrap();
    encoded.pop();
    assert!(ColumnarBlockCodec::decode(&encoded).is_err());

    let mut vector = VectorGraphCodec::encode(&VectorGraphNode {
        record_id: 1,
        vector: vec![1.0, 2.0],
        neighbors: vec![2, 3],
    })
    .unwrap();
    vector.pop();
    assert!(VectorGraphCodec::decode(&vector).is_err());
}
