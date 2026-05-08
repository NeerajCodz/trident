use trident::identity::{Aid, Cid, Eid, Fid, FieldId, Pid, Rid, Sid, SlotAddress};
use trident::page::RecordPage;
use trident::record::{RecordSlot, RidDirectory};

#[test]
fn record_page_inserts_reads_deletes_and_defragments_slots() {
    let mut page = RecordPage::new(10);
    page.insert(Sid(1), b"alpha").unwrap();
    page.insert(Sid(2), b"bravo").unwrap();

    assert_eq!(page.get(Sid(1)).unwrap(), Some(b"alpha".as_slice()));
    assert_eq!(page.get(Sid(2)).unwrap(), Some(b"bravo".as_slice()));
    assert_eq!(page.header.slot_count, 2);

    page.delete(Sid(1)).unwrap();
    assert_eq!(page.get(Sid(1)).unwrap(), None);
    page.defragment();

    assert_eq!(page.slots().len(), 1);
    assert_eq!(page.get(Sid(2)).unwrap(), Some(b"bravo".as_slice()));
}

#[test]
fn record_page_roundtrips_with_checksum_and_rejects_corruption() {
    let mut page = RecordPage::new(99);
    page.insert(Sid(7), b"payload").unwrap();

    let encoded = page.to_bytes();
    let decoded = RecordPage::from_bytes(&encoded, "page").unwrap();
    assert_eq!(decoded.header.page_lsn, 99);
    assert_eq!(decoded.get(Sid(7)).unwrap(), Some(b"payload".as_slice()));

    let mut corrupt = encoded;
    let last = corrupt.len() - 1;
    corrupt[last] ^= 0xff;
    assert!(RecordPage::from_bytes(&corrupt, "page").is_err());
}

#[test]
fn record_slot_tracks_field_offsets_and_rid_directory_resolves_vids() {
    let slot = RecordSlot::from_fields(
        Sid(0x001a),
        &[
            (FieldId::Fixed(Aid(1)), b"Neeraj".as_slice()),
            (FieldId::Fixed(Aid(3)), &92_i32.to_le_bytes()),
        ],
    );
    assert_eq!(
        slot.field_bytes(FieldId::Fixed(Aid(1))).unwrap(),
        Some(b"Neeraj".as_slice())
    );

    let mut directory = RidDirectory::default();
    directory.insert(
        Rid(42),
        SlotAddress {
            cid: Cid(1),
            eid: Eid(2),
            fid: Fid(0x000f),
            pid: Pid(3),
            sid: Sid(0x001a),
        },
    );

    let vid = directory.vid_for(Rid(42), FieldId::Fixed(Aid(3))).unwrap();
    assert_eq!(vid.to_global_hex(), "0002-000f-0003-001a-0003");

    let mut page = RecordPage::new(1);
    slot.install_into_page(&mut page).unwrap();
    assert_eq!(page.get(Sid(0x001a)).unwrap(), Some(slot.body.as_slice()));
}
