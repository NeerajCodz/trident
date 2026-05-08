use crate::errors::{Result, TridentError};
use crate::identity::{Cid, Rid};
use serde::{Deserialize, Serialize};

pub const INLINE_VARLEN_THRESHOLD: usize = 256;
pub const INLINE_LIST_THRESHOLD: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StorageClass {
    Inline,
    External,
    Segment,
    Catalog,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SegmentFamily {
    Vector,
    Edge,
    FullText,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[repr(u8)]
pub enum TypeCode {
    Int2 = 0x01,
    Int4 = 0x02,
    Int8 = 0x03,
    UInt8 = 0x04,
    Float4 = 0x10,
    Float8 = 0x11,
    Numeric = 0x12,
    Bool = 0x20,
    TextInline = 0x30,
    Json = 0x31,
    Jsonb = 0x32,
    Uuid = 0x40,
    Date = 0x50,
    Time = 0x51,
    Timestamp = 0x53,
    Money = 0x60,
    Enum = 0x61,
    Bytea = 0x70,
    List = 0x80,
    Set = 0x81,
    Dict = 0x82,
    Range = 0x83,
    Vec32 = 0x90,
    Vec16 = 0x91,
    Vec8 = 0x92,
    VecBin = 0x93,
    Embedding = 0x94,
    SparseVec = 0x95,
    EdgeRef = 0xA0,
    EdgeList = 0xA1,
    Path = 0xA2,
    Subgraph = 0xA3,
    TsVector = 0xB0,
    GeoPoint = 0xC0,
    GeoShape = 0xC1,
    Cidr = 0xD0,
    Inet = 0xD1,
    CommitRef = 0xE0,
    BranchRef = 0xE1,
    RidRef = 0xE2,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ExternalPointer {
    Overflow {
        page_id: u64,
        offset: u32,
        len: u32,
        checksum: u32,
    },
    Segment {
        family: SegmentFamily,
        segment_id: u64,
        offset: u64,
        len: u32,
    },
    Catalog {
        catalog: String,
        key: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncodedValue {
    pub type_code: TypeCode,
    pub storage_class: StorageClass,
    pub inline_bytes: Vec<u8>,
    pub pointer: Option<ExternalPointer>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TridentValue {
    Int2(i16),
    Int4(i32),
    Int8(i64),
    UInt8(u64),
    Float4(f32),
    Float8(f64),
    Bool(bool),
    Text(String),
    Json(Vec<u8>),
    Jsonb(Vec<u8>),
    Uuid([u8; 16]),
    Date(i32),
    TimeMicros(i64),
    TimestampMicros(i64),
    Money {
        amount: i64,
        currency: [u8; 3],
        scale: u8,
    },
    Enum(u16),
    Bytea(Vec<u8>),
    List {
        element_type: TypeCode,
        encoded_elements: Vec<u8>,
    },
    Vec32 {
        dims: u16,
        data: Vec<f32>,
    },
    EdgeRef {
        source: Rid,
        target: Rid,
        edge_type: u32,
        weight_bits: u32,
        created_at: u64,
    },
    TsVector(Vec<u8>),
    RidRef {
        collection: Cid,
        rid: Rid,
    },
}

pub struct DataTypeRegistry;

impl DataTypeRegistry {
    pub fn encode(value: &TridentValue) -> Result<EncodedValue> {
        match value {
            TridentValue::Int2(value) => inline(TypeCode::Int2, value.to_le_bytes()),
            TridentValue::Int4(value) => inline(TypeCode::Int4, value.to_le_bytes()),
            TridentValue::Int8(value) => inline(TypeCode::Int8, value.to_le_bytes()),
            TridentValue::UInt8(value) => inline(TypeCode::UInt8, value.to_le_bytes()),
            TridentValue::Float4(value) => inline(TypeCode::Float4, value.to_le_bytes()),
            TridentValue::Float8(value) => inline(TypeCode::Float8, value.to_le_bytes()),
            TridentValue::Bool(value) => inline(TypeCode::Bool, [u8::from(*value)]),
            TridentValue::Uuid(value) => inline(TypeCode::Uuid, *value),
            TridentValue::Date(value) => inline(TypeCode::Date, value.to_le_bytes()),
            TridentValue::TimeMicros(value) => inline(TypeCode::Time, value.to_le_bytes()),
            TridentValue::TimestampMicros(value) => {
                inline(TypeCode::Timestamp, value.to_le_bytes())
            }
            TridentValue::Money {
                amount,
                currency,
                scale,
            } => {
                let mut bytes = Vec::with_capacity(12);
                bytes.extend_from_slice(&amount.to_le_bytes());
                bytes.extend_from_slice(currency);
                bytes.push(*scale);
                Ok(inline_vec(TypeCode::Money, bytes))
            }
            TridentValue::Enum(value) => inline(TypeCode::Enum, value.to_le_bytes()),
            TridentValue::RidRef { collection, rid } => {
                let mut bytes = Vec::with_capacity(12);
                bytes.extend_from_slice(&collection.0.to_le_bytes());
                bytes.extend_from_slice(&rid.0.to_le_bytes());
                Ok(inline_vec(TypeCode::RidRef, bytes))
            }
            TridentValue::Text(value) => varlen(TypeCode::TextInline, value.as_bytes()),
            TridentValue::Bytea(value) => varlen(TypeCode::Bytea, value),
            TridentValue::Json(value) => external(TypeCode::Json, value.len() as u32),
            TridentValue::Jsonb(value) => external(TypeCode::Jsonb, value.len() as u32),
            TridentValue::List {
                element_type,
                encoded_elements,
            } => {
                let mut bytes = Vec::with_capacity(5 + encoded_elements.len());
                bytes.extend_from_slice(&(encoded_elements.len() as u32).to_le_bytes());
                bytes.push(*element_type as u8);
                bytes.extend_from_slice(encoded_elements);
                if bytes.len() <= INLINE_LIST_THRESHOLD {
                    Ok(inline_vec(TypeCode::List, bytes))
                } else {
                    external(TypeCode::List, bytes.len() as u32)
                }
            }
            TridentValue::Vec32 { dims, data } => {
                if *dims as usize != data.len() {
                    return Err(TridentError::InvalidConfig(
                        "vec32 dimensions do not match data length".to_string(),
                    ));
                }
                segment(
                    TypeCode::Vec32,
                    SegmentFamily::Vector,
                    data.len() as u32 * 4,
                )
            }
            TridentValue::EdgeRef { .. } => segment(TypeCode::EdgeRef, SegmentFamily::Edge, 32),
            TridentValue::TsVector(bytes) => segment(
                TypeCode::TsVector,
                SegmentFamily::FullText,
                bytes.len() as u32,
            ),
        }
    }
}

impl TridentValue {
    pub fn payload_bytes(&self) -> Result<Vec<u8>> {
        match self {
            Self::Int2(value) => Ok(value.to_le_bytes().to_vec()),
            Self::Int4(value) => Ok(value.to_le_bytes().to_vec()),
            Self::Int8(value) => Ok(value.to_le_bytes().to_vec()),
            Self::UInt8(value) => Ok(value.to_le_bytes().to_vec()),
            Self::Float4(value) => Ok(value.to_le_bytes().to_vec()),
            Self::Float8(value) => Ok(value.to_le_bytes().to_vec()),
            Self::Bool(value) => Ok(vec![u8::from(*value)]),
            Self::Text(value) => Ok(value.as_bytes().to_vec()),
            Self::Json(value) | Self::Jsonb(value) | Self::Bytea(value) | Self::TsVector(value) => {
                Ok(value.clone())
            }
            Self::Uuid(value) => Ok(value.to_vec()),
            Self::Date(value) => Ok(value.to_le_bytes().to_vec()),
            Self::TimeMicros(value) | Self::TimestampMicros(value) => {
                Ok(value.to_le_bytes().to_vec())
            }
            Self::Money {
                amount,
                currency,
                scale,
            } => {
                let mut bytes = Vec::with_capacity(12);
                bytes.extend_from_slice(&amount.to_le_bytes());
                bytes.extend_from_slice(currency);
                bytes.push(*scale);
                Ok(bytes)
            }
            Self::Enum(value) => Ok(value.to_le_bytes().to_vec()),
            Self::List {
                element_type,
                encoded_elements,
            } => {
                let mut bytes = Vec::with_capacity(5 + encoded_elements.len());
                bytes.extend_from_slice(&(encoded_elements.len() as u32).to_le_bytes());
                bytes.push(*element_type as u8);
                bytes.extend_from_slice(encoded_elements);
                Ok(bytes)
            }
            Self::Vec32 { dims, data } => {
                if *dims as usize != data.len() {
                    return Err(TridentError::InvalidConfig(
                        "vec32 dimensions do not match data length".to_string(),
                    ));
                }
                let mut bytes = Vec::with_capacity(2 + data.len() * 4);
                bytes.extend_from_slice(&dims.to_le_bytes());
                for value in data {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
                Ok(bytes)
            }
            Self::EdgeRef {
                source,
                target,
                edge_type,
                weight_bits,
                created_at,
            } => {
                let mut bytes = Vec::with_capacity(32);
                bytes.extend_from_slice(&source.0.to_le_bytes());
                bytes.extend_from_slice(&target.0.to_le_bytes());
                bytes.extend_from_slice(&edge_type.to_le_bytes());
                bytes.extend_from_slice(&weight_bits.to_le_bytes());
                bytes.extend_from_slice(&created_at.to_le_bytes());
                Ok(bytes)
            }
            Self::RidRef { collection, rid } => {
                let mut bytes = Vec::with_capacity(12);
                bytes.extend_from_slice(&collection.0.to_le_bytes());
                bytes.extend_from_slice(&rid.0.to_le_bytes());
                Ok(bytes)
            }
        }
    }
}

fn inline<const N: usize>(type_code: TypeCode, bytes: [u8; N]) -> Result<EncodedValue> {
    Ok(inline_vec(type_code, bytes.to_vec()))
}

fn inline_vec(type_code: TypeCode, inline_bytes: Vec<u8>) -> EncodedValue {
    EncodedValue {
        type_code,
        storage_class: StorageClass::Inline,
        inline_bytes,
        pointer: None,
    }
}

fn varlen(type_code: TypeCode, bytes: &[u8]) -> Result<EncodedValue> {
    if bytes.len() <= INLINE_VARLEN_THRESHOLD {
        let mut inline_bytes = Vec::with_capacity(4 + bytes.len());
        inline_bytes.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        inline_bytes.extend_from_slice(bytes);
        Ok(inline_vec(type_code, inline_bytes))
    } else {
        external(type_code, bytes.len() as u32)
    }
}

fn external(type_code: TypeCode, len: u32) -> Result<EncodedValue> {
    Ok(EncodedValue {
        type_code,
        storage_class: StorageClass::External,
        inline_bytes: Vec::new(),
        pointer: Some(ExternalPointer::Overflow {
            page_id: 0,
            offset: 0,
            len,
            checksum: 0,
        }),
    })
}

fn segment(type_code: TypeCode, family: SegmentFamily, len: u32) -> Result<EncodedValue> {
    Ok(EncodedValue {
        type_code,
        storage_class: StorageClass::Segment,
        inline_bytes: Vec::new(),
        pointer: Some(ExternalPointer::Segment {
            family,
            segment_id: 0,
            offset: 0,
            len,
        }),
    })
}
