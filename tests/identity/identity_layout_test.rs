use praxis::identity::{
    Aid, Cid, Did, Eid, Fid, FieldId, Pid, RelativeVid, Sid, SlotAddress, Vid, VidContext,
};
use praxis::layout::{DurableLayoutKind, PraxisLayout};
use tempfile::tempdir;

#[test]
fn ids_format_as_compact_hex() {
    assert_eq!(Cid(1).to_string(), "0001");
    assert_eq!(Eid(0x00af).to_hex(), "00af");
    assert_eq!(praxis::Rid(0x1abc).to_string(), "0000000000001abc");
}

#[test]
fn relative_vid_resolution_requires_deterministic_context() {
    let relative = RelativeVid::PageRelative {
        sid: Sid(0x001a),
        field: FieldId::Fixed(Aid(0x0003)),
    };

    assert!(relative.resolve(VidContext::default()).is_err());

    let resolved = relative
        .resolve(VidContext {
            eid: Some(Eid(1)),
            fid: Some(Fid(0x000f)),
            pid: Some(Pid(2)),
        })
        .unwrap();

    assert_eq!(
        resolved,
        Vid::sql(Eid(1), Fid(0x000f), Pid(2), Sid(0x001a), Aid(3))
    );
    assert_eq!(resolved.to_global_hex(), "0001-000f-0002-001a-0003");
    assert_eq!(
        resolved
            .to_entity_relative()
            .resolve(VidContext {
                eid: Some(Eid(1)),
                ..VidContext::default()
            })
            .unwrap(),
        resolved
    );
    assert_eq!(
        resolved
            .to_frame_relative()
            .resolve(VidContext {
                eid: Some(Eid(1)),
                fid: Some(Fid(0x000f)),
                pid: None,
            })
            .unwrap(),
        resolved
    );
    assert_eq!(
        resolved
            .to_page_relative()
            .resolve(VidContext {
                eid: Some(Eid(1)),
                fid: Some(Fid(0x000f)),
                pid: Some(Pid(2)),
            })
            .unwrap(),
        resolved
    );
}

#[test]
fn sql_and_nosql_vids_keep_field_identity_separate() {
    let sql = Vid::sql(Eid(1), Fid(2), Pid(3), Sid(4), Aid(5));
    let nosql = Vid::nosql(Eid(1), Fid(2), Pid(3), Sid(4), Did(5));

    assert!(sql.field.is_fixed());
    assert!(!nosql.field.is_fixed());
    assert_ne!(sql.field, nosql.field);

    let from_slot = Vid::from_slot(
        SlotAddress {
            cid: Cid(7),
            eid: Eid(1),
            fid: Fid(2),
            pid: Pid(3),
            sid: Sid(4),
        },
        FieldId::Fixed(Aid(5)),
    );
    assert_eq!(from_slot, sql);
}

#[test]
fn praxis_layout_creates_canonical_tree_and_paths() {
    let dir = tempdir().unwrap();
    let layout = PraxisLayout::new(dir.path());
    layout.create_all().unwrap();
    layout.ensure_entity_tree(Cid(1), Eid(2)).unwrap();

    assert!(layout.root().ends_with(".praxis"));
    assert!(layout.catalog_root().exists());
    assert!(layout.catalog_root().join("indexes.cat").exists());
    assert!(layout.catalog_root().join("vector_models.cat").exists());
    assert!(layout.catalog_root().join("fulltext.cat").exists());
    assert!(layout.catalog_root().join("graph.cat").exists());
    assert!(layout.catalog_root().join("shards.cat").exists());
    assert!(layout.catalog_root().join("users.cat").exists());
    assert!(layout.config_root().exists());
    assert!(layout.config_root().join("engine.toml").exists());
    assert!(layout.runtime_root().join("sockets").exists());
    assert!(layout.logs_root().join("recovery.log").exists());
    assert!(
        layout
            .collection_root(Cid(1))
            .join("meta")
            .join("collection.meta")
            .exists()
    );
    assert!(
        layout
            .collection_root(Cid(1))
            .join("wal")
            .join("active")
            .exists()
    );
    assert!(
        layout
            .collection_root(Cid(1))
            .join("kv")
            .join("critical")
            .join("rid_to_page.kv")
            .exists()
    );
    assert!(
        layout
            .collection_root(Cid(1))
            .join("pit")
            .join("commits")
            .exists()
    );
    assert!(
        layout
            .collection_root(Cid(1))
            .join("tiers")
            .join("hot")
            .exists()
    );
    assert!(
        layout
            .collection_root(Cid(1))
            .join("ops")
            .join("transactions")
            .exists()
    );
    assert!(
        layout
            .collection_root(Cid(1))
            .join("tmp")
            .join("compaction")
            .exists()
    );
    assert!(layout.entity_root(Cid(1), Eid(2)).exists());
    assert!(
        layout
            .entity_root(Cid(1), Eid(2))
            .join("entity.meta")
            .exists()
    );
    assert!(
        layout
            .entity_root(Cid(1), Eid(2))
            .join("indexes")
            .join("vector")
            .join("hnsw.idx")
            .exists()
    );
    assert!(
        layout
            .entity_root(Cid(1), Eid(2))
            .join("graph")
            .join("adjacency")
            .exists()
    );

    let page = layout.page_path(Cid(1), Eid(2), Fid(0x000f), Pid(3));
    assert_eq!(page.kind, DurableLayoutKind::Page);
    assert!(page.manifest_tracked);
    assert!(page.path.ends_with("0003.page"));
    assert!(page.path.to_string_lossy().contains("0001"));
    assert!(page.path.to_string_lossy().contains("0002"));
    layout
        .ensure_frame_tree(Cid(1), Eid(2), Fid(0x000f))
        .unwrap();
    assert!(
        layout
            .frame_root(Cid(1), Eid(2), Fid(0x000f))
            .join("frame.meta")
            .exists()
    );

    let critical = layout.critical_map_path(Cid(1), "rid_to_slot");
    assert!(critical.path.ends_with("rid_to_slot.kv"));

    let active_wal = layout.collection_active_wal_path(Cid(1), 1);
    assert!(active_wal.path.ends_with("00000001.wal"));
    assert!(active_wal.path.to_string_lossy().contains("active"));

    let overflow = layout.overflow_blob_path(Cid(1), Eid(2));
    assert!(overflow.path.to_string_lossy().contains("overflow"));
    assert!(overflow.path.ends_with("000001.ovf"));
}
