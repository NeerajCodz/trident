use crate::config::Compression;
use crate::identity::{Aid, Cid, Did, Eid};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum EntityStorageModel {
    Sql,
    NoSql,
    Graph,
    Vector,
    FullText,
    Hybrid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CollectionCatalogEntry {
    pub cid: Cid,
    pub name: String,
    pub retention_policy: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EntityCatalogEntry {
    pub cid: Cid,
    pub eid: Eid,
    pub name: String,
    pub storage_model: EntityStorageModel,
    pub primary_key: Option<String>,
    pub compression: Compression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FixedAttributeCatalogEntry {
    pub eid: Eid,
    pub aid: Aid,
    pub name: String,
    pub type_code: u8,
    pub nullable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DynamicAttributeCatalogEntry {
    pub eid: Eid,
    pub did: Did,
    pub name: String,
    pub inferred_type_code: Option<u8>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogSnapshot {
    pub collections: Vec<CollectionCatalogEntry>,
    pub entities: Vec<EntityCatalogEntry>,
    pub fixed_attributes: Vec<FixedAttributeCatalogEntry>,
    pub dynamic_attributes: Vec<DynamicAttributeCatalogEntry>,
}
