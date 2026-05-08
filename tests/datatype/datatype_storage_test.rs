use trident::datatype::{
    DataTypeRegistry, ExternalPointer, SegmentFamily, StorageClass, TridentValue, TypeCode,
};
use trident::identity::{Cid, Rid};

#[test]
fn scalar_values_encode_inline_little_endian() {
    let encoded = DataTypeRegistry::encode(&TridentValue::Int4(92)).unwrap();

    assert_eq!(encoded.type_code, TypeCode::Int4);
    assert_eq!(encoded.storage_class, StorageClass::Inline);
    assert_eq!(encoded.inline_bytes, 92_i32.to_le_bytes());
}

#[test]
fn text_and_bytea_follow_inline_threshold() {
    let small = DataTypeRegistry::encode(&TridentValue::Text("hello".into())).unwrap();
    let large = DataTypeRegistry::encode(&TridentValue::Bytea(vec![7; 257])).unwrap();

    assert_eq!(small.storage_class, StorageClass::Inline);
    assert_eq!(large.storage_class, StorageClass::External);
    assert!(matches!(
        large.pointer,
        Some(ExternalPointer::Overflow { len: 257, .. })
    ));
}

#[test]
fn json_is_external_even_when_small() {
    let encoded = DataTypeRegistry::encode(&TridentValue::Json(br#"{"a":1}"#.to_vec())).unwrap();

    assert_eq!(encoded.storage_class, StorageClass::External);
    assert_eq!(encoded.type_code, TypeCode::Json);
}

#[test]
fn vectors_edges_and_fulltext_use_segment_storage() {
    let vector = DataTypeRegistry::encode(&TridentValue::Vec32 {
        dims: 3,
        data: vec![1.0, 2.0, 3.0],
    })
    .unwrap();
    let edge = DataTypeRegistry::encode(&TridentValue::EdgeRef {
        source: Rid(1),
        target: Rid(2),
        edge_type: 7,
        weight_bits: 1.0_f32.to_bits(),
        created_at: 9,
    })
    .unwrap();
    let text = DataTypeRegistry::encode(&TridentValue::TsVector(b"token".to_vec())).unwrap();

    assert!(matches!(
        vector.pointer,
        Some(ExternalPointer::Segment {
            family: SegmentFamily::Vector,
            ..
        })
    ));
    assert!(matches!(
        edge.pointer,
        Some(ExternalPointer::Segment {
            family: SegmentFamily::Edge,
            ..
        })
    ));
    assert!(matches!(
        text.pointer,
        Some(ExternalPointer::Segment {
            family: SegmentFamily::FullText,
            ..
        })
    ));
}

#[test]
fn rid_ref_is_inline_collection_and_record_identity() {
    let encoded = DataTypeRegistry::encode(&TridentValue::RidRef {
        collection: Cid(3),
        rid: Rid(42),
    })
    .unwrap();

    assert_eq!(encoded.type_code, TypeCode::RidRef);
    assert_eq!(encoded.storage_class, StorageClass::Inline);
    assert_eq!(encoded.inline_bytes.len(), 12);
}
