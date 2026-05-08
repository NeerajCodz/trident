use tempfile::tempdir;
use trident::identity::{Aid, Cid, Did, Eid, Fid, FieldId, Pid, RelativeVid, Sid, Vid, VidContext};
use trident::layout::{DurableLayoutKind, TridentLayout};

#[test]
fn ids_format_as_compact_hex() {
    assert_eq!(Cid(1).to_string(), "0001");
    assert_eq!(Eid(0x00af).to_hex(), "00af");
    assert_eq!(trident::Rid(0x1abc).to_string(), "0000000000001abc");
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
}

#[test]
fn sql_and_nosql_vids_keep_field_identity_separate() {
    let sql = Vid::sql(Eid(1), Fid(2), Pid(3), Sid(4), Aid(5));
    let nosql = Vid::nosql(Eid(1), Fid(2), Pid(3), Sid(4), Did(5));

    assert!(sql.field.is_fixed());
    assert!(!nosql.field.is_fixed());
    assert_ne!(sql.field, nosql.field);
}

#[test]
fn trident_layout_creates_canonical_tree_and_paths() {
    let dir = tempdir().unwrap();
    let layout = TridentLayout::new(dir.path());
    layout.create_all().unwrap();
    layout.ensure_entity_tree(Cid(1), Eid(2)).unwrap();

    assert!(layout.root().ends_with(".trident"));
    assert!(layout.catalog_root().exists());
    assert!(layout.config_root().exists());
    assert!(layout.entity_root(Cid(1), Eid(2)).exists());

    let page = layout.page_path(Cid(1), Eid(2), Fid(0x000f), Pid(3));
    assert_eq!(page.kind, DurableLayoutKind::Page);
    assert!(page.manifest_tracked);
    assert!(page.path.ends_with("0003.page"));
    assert!(page.path.to_string_lossy().contains("0001"));
    assert!(page.path.to_string_lossy().contains("0002"));

    let critical = layout.critical_map_path(Cid(1), "rid_to_slot");
    assert!(critical.path.ends_with("rid_to_slot.kv"));
}
