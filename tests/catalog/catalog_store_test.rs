use tempfile::tempdir;
use trident::catalog::{
    CatalogSnapshot, CatalogStore, CollectionCatalogEntry, DynamicAttributeCatalogEntry,
    EntityCatalogEntry, EntityStorageModel, FixedAttributeCatalogEntry,
};
use trident::config::Compression;
use trident::identity::{Aid, Cid, Did, Eid};

#[test]
fn catalog_store_loads_empty_doc_shape_catalog_files() {
    let dir = tempdir().unwrap();
    let store = CatalogStore::open(dir.path()).unwrap();

    assert_eq!(store.load().unwrap(), CatalogSnapshot::default());
    assert!(store.layout().catalog_root().join("indexes.cat").exists());
    assert!(
        store
            .layout()
            .catalog_root()
            .join("vector_models.cat")
            .exists()
    );
    assert!(store.layout().catalog_root().join("fulltext.cat").exists());
    assert!(store.layout().catalog_root().join("graph.cat").exists());
}

#[test]
fn catalog_store_persists_collection_entity_and_attribute_catalogs() {
    let dir = tempdir().unwrap();
    let store = CatalogStore::open(dir.path()).unwrap();
    let snapshot = CatalogSnapshot {
        collections: vec![CollectionCatalogEntry {
            cid: Cid(1),
            name: "users".into(),
            retention_policy: Some("hot-30d".into()),
        }],
        entities: vec![EntityCatalogEntry {
            cid: Cid(1),
            eid: Eid(2),
            name: "students".into(),
            storage_model: EntityStorageModel::Hybrid,
            primary_key: Some("id".into()),
            compression: Compression::Lz4,
        }],
        fixed_attributes: vec![FixedAttributeCatalogEntry {
            eid: Eid(2),
            aid: Aid(3),
            name: "marks".into(),
            type_code: 0x02,
            nullable: false,
        }],
        dynamic_attributes: vec![DynamicAttributeCatalogEntry {
            eid: Eid(2),
            did: Did(0x00af),
            name: "teacher".into(),
            inferred_type_code: Some(0x30),
        }],
    };

    store.save(&snapshot).unwrap();
    let loaded = CatalogStore::open(dir.path()).unwrap().load().unwrap();

    assert_eq!(loaded, snapshot);
    assert!(
        store
            .layout()
            .catalog_root()
            .join("collections.cat")
            .exists()
    );
    assert!(store.layout().catalog_root().join("entities.cat").exists());
}

#[test]
fn catalog_store_rejects_corrupted_catalog_payload() {
    let dir = tempdir().unwrap();
    let store = CatalogStore::open(dir.path()).unwrap();
    store
        .save(&CatalogSnapshot {
            collections: vec![CollectionCatalogEntry {
                cid: Cid(1),
                name: "users".into(),
                retention_policy: None,
            }],
            ..CatalogSnapshot::default()
        })
        .unwrap();

    let path = store.layout().catalog_root().join("collections.cat");
    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();

    assert!(store.load().is_err());
}
