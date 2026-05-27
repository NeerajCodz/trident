use crate::datatype::{
    DataTypeRegistry, ExternalPointer, PraxisValue, SegmentFamily, StorageClass, TypeCode,
};
use crate::errors::{PraxisError, Result};
use crate::identity::{Aid, Cid, Did, Eid, Fid, FieldId, Pid, Rid, Sid, SlotAddress, Vid};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use crate::layout::PraxisLayout;
use crate::manifest::PageManifestStore;
use crate::page::{RecordPage, SlotDirectoryEntry};
use crate::segments::{BlobLocation, BlobStore};
use crate::wal::{PageWal, PageWalMutation, PageWalRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

const RID_DIRECTORY_MAGIC: u32 = 0x5452_4944;
const RID_DIRECTORY_VERSION: u8 = 1;
const TYPED_RECORD_MAGIC: u32 = 0x5456_4944;
const TYPED_RECORD_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FieldOffset {
    pub field: FieldId,
    pub offset: u32,
    pub len: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordSlot {
    pub sid: Sid,
    pub body: Vec<u8>,
    pub fields: Vec<FieldOffset>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RidDirectory {
    mappings: BTreeMap<Rid, SlotAddress>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageRecordStore {
    layout: PraxisLayout,
    cid: Cid,
    eid: Eid,
    fid: Fid,
    current_pid: Pid,
    next_rid: u64,
    next_sid: u16,
    directory: RidDirectory,
}

impl RecordSlot {
    pub fn from_fields(sid: Sid, fields: &[(FieldId, &[u8])]) -> Self {
        let mut body = Vec::new();
        let mut offsets = Vec::with_capacity(fields.len());
        for (field, bytes) in fields {
            let offset = body.len() as u32;
            body.extend_from_slice(bytes);
            offsets.push(FieldOffset {
                field: *field,
                offset,
                len: bytes.len() as u32,
            });
        }
        Self {
            sid,
            body,
            fields: offsets,
        }
    }

    pub fn fixed(sid: Sid, aid: Aid, value: &[u8]) -> Self {
        Self::from_fields(sid, &[(FieldId::Fixed(aid), value)])
    }

    pub fn dynamic(sid: Sid, did: Did, value: &[u8]) -> Self {
        Self::from_fields(sid, &[(FieldId::Dynamic(did), value)])
    }

    pub fn field_bytes(&self, field: FieldId) -> Result<Option<&[u8]>> {
        let Some(offset) = self.fields.iter().find(|offset| offset.field == field) else {
            return Ok(None);
        };
        let start = offset.offset as usize;
        let end = start.saturating_add(offset.len as usize);
        if end > self.body.len() {
            return Err(PraxisError::Corrupt {
                path: "record-slot".into(),
                reason: "field offset points outside record slot body".to_string(),
            });
        }
        Ok(Some(&self.body[start..end]))
    }

    pub fn install_into_page(&self, page: &mut RecordPage) -> Result<()> {
        page.insert(self.sid, &self.body)
    }
}

impl RidDirectory {
    pub fn insert(&mut self, rid: Rid, address: SlotAddress) {
        self.mappings.insert(rid, address);
    }

    pub fn resolve(&self, rid: Rid) -> Result<SlotAddress> {
        self.mappings
            .get(&rid)
            .copied()
            .ok_or(PraxisError::KeyNotFound)
    }

    pub fn relocate(&mut self, rid: Rid, address: SlotAddress) -> Result<()> {
        if !self.mappings.contains_key(&rid) {
            return Err(PraxisError::KeyNotFound);
        }
        self.mappings.insert(rid, address);
        Ok(())
    }

    pub fn vid_for(&self, rid: Rid, field: FieldId) -> Result<Vid> {
        let address = self.resolve(rid)?;
        Ok(Vid {
            eid: address.eid,
            fid: address.fid,
            pid: address.pid,
            sid: address.sid,
            field,
        })
    }

    pub fn save_binary(
        &self,
        path: &Path,
        next_rid: u64,
        next_sid: u16,
        current_pid: Pid,
    ) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut payload = BinaryWriter::new();
        payload.write_u64(next_rid);
        payload.write_u32(next_sid as u32);
        payload.write_u32(current_pid.0);
        payload.write_u32(self.mappings.len() as u32);
        for (rid, address) in &self.mappings {
            payload.write_u64(rid.0);
            payload.write_u32(address.cid.0);
            payload.write_u32(address.eid.0);
            payload.write_u32(address.fid.0);
            payload.write_u32(address.pid.0);
            payload.write_u32(address.sid.0 as u32);
        }
        let payload = payload.into_inner();
        let mut out = BinaryWriter::new();
        out.write_u32(RID_DIRECTORY_MAGIC);
        out.write_u8(RID_DIRECTORY_VERSION);
        out.write_u32(payload.len() as u32);
        out.write_u32(crc32c(&payload));
        out.write_bytes(&payload);
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, out.into_inner())?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&tmp_path)?
            .sync_all()?;
        #[cfg(windows)]
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    pub fn load_binary(path: &Path) -> Result<(Self, u64, u16, Pid)> {
        let bytes = std::fs::read(path)?;
        if bytes.len() < 13 {
            return corrupt(path, "truncated RID directory");
        }
        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != RID_DIRECTORY_MAGIC {
            return corrupt(path, "bad RID directory magic");
        }
        if bytes[4] != RID_DIRECTORY_VERSION {
            return corrupt(
                path,
                &format!("unsupported RID directory version {}", bytes[4]),
            );
        }
        let payload_len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
        let expected = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
        if bytes.len() < 13 + payload_len {
            return corrupt(path, "truncated RID directory payload");
        }
        let payload = &bytes[13..13 + payload_len];
        let actual = crc32c(payload);
        if actual != expected {
            return corrupt(
                path,
                &format!(
                    "RID directory checksum mismatch: expected {expected:#010x}, got {actual:#010x}"
                ),
            );
        }
        let mut reader = BinaryReader::new(payload, path.to_path_buf());
        let next_rid = reader.read_u64()?;
        let next_sid = reader.read_u32()? as u16;
        let current_pid = Pid(reader.read_u32()?);
        let count = reader.read_u32()? as usize;
        let mut directory = Self::default();
        for _ in 0..count {
            let rid = Rid(reader.read_u64()?);
            let address = SlotAddress {
                cid: Cid(reader.read_u32()?),
                eid: Eid(reader.read_u32()?),
                fid: Fid(reader.read_u32()?),
                pid: Pid(reader.read_u32()?),
                sid: Sid(reader.read_u32()? as u16),
            };
            directory.insert(rid, address);
        }
        Ok((directory, next_rid, next_sid, current_pid))
    }
}

impl PageRecordStore {
    pub fn open(base: impl Into<PathBuf>, cid: Cid, eid: Eid) -> Result<Self> {
        let layout = PraxisLayout::new(base);
        layout.create_all()?;
        layout.ensure_entity_tree(cid, eid)?;
        let map_path = layout.critical_map_path(cid, "rid_to_slot").path;
        let (directory, next_rid, next_sid, current_pid) =
            if map_path.exists() && map_path.metadata()?.len() > 0 {
                RidDirectory::load_binary(&map_path)?
            } else {
                (RidDirectory::default(), 1, 1, Pid(1))
            };
        Ok(Self {
            layout,
            cid,
            eid,
            fid: Fid(1),
            current_pid,
            next_rid,
            next_sid,
            directory,
        })
    }

    pub fn put(&mut self, fields: &[(FieldId, &[u8])]) -> Result<Rid> {
        let rid = Rid(self.next_rid);
        self.apply_put(rid, fields)?;
        Ok(rid)
    }

    pub fn put_durable(&mut self, wal: &mut PageWal, fields: &[(FieldId, &[u8])]) -> Result<Rid> {
        let rid = Rid(self.next_rid);
        wal.append_put(self.cid, self.eid, rid, fields)?;
        self.apply_put(rid, fields)?;
        Ok(rid)
    }

    pub fn delete_durable(&mut self, wal: &mut PageWal, rid: Rid) -> Result<()> {
        wal.append_delete(self.cid, self.eid, rid)?;
        self.delete(rid)
    }

    pub fn put_typed(&mut self, fields: &[(FieldId, PraxisValue)]) -> Result<Rid> {
        let encoded = self.encode_typed_record(fields)?;
        self.put(&[(FieldId::Dynamic(Did(0xffff)), encoded.as_slice())])
    }

    pub fn get_typed_field_bytes(&self, rid: Rid, field: FieldId) -> Result<Vec<u8>> {
        let body = self.get(rid)?;
        let cells = decode_typed_record(&body, Path::new("typed-record"))?;
        let cell = cells
            .into_iter()
            .find(|cell| cell.field == field)
            .ok_or(PraxisError::KeyNotFound)?;
        match cell.storage_class {
            StorageClass::Inline => Ok(cell.inline_bytes),
            StorageClass::External => {
                let ExternalPointer::Overflow {
                    page_id,
                    offset,
                    len,
                    checksum,
                } = cell.pointer.ok_or_else(|| PraxisError::Corrupt {
                    path: PathBuf::from("typed-record"),
                    reason: "external typed cell is missing overflow pointer".to_string(),
                })?
                else {
                    return Err(PraxisError::Corrupt {
                        path: PathBuf::from("typed-record"),
                        reason: "external typed cell has non-overflow pointer".to_string(),
                    });
                };
                self.overflow_store()?.read(&BlobLocation {
                    family: None,
                    file_id: page_id,
                    offset: offset as u64,
                    len,
                    checksum,
                })
            }
            StorageClass::Segment => {
                let ExternalPointer::Segment {
                    family,
                    segment_id,
                    offset,
                    len,
                } = cell.pointer.ok_or_else(|| PraxisError::Corrupt {
                    path: PathBuf::from("typed-record"),
                    reason: "segment typed cell is missing segment pointer".to_string(),
                })?
                else {
                    return Err(PraxisError::Corrupt {
                        path: PathBuf::from("typed-record"),
                        reason: "segment typed cell has non-segment pointer".to_string(),
                    });
                };
                self.segment_store(family)?.read(&BlobLocation {
                    family: Some(family),
                    file_id: segment_id,
                    offset,
                    len,
                    checksum: 0,
                })
            }
            StorageClass::Catalog => Err(PraxisError::InvalidConfig(
                "catalog-backed values are resolved through catalog stores".to_string(),
            )),
        }
    }

    pub fn replay_page_wal(&mut self, records: &[PageWalRecord]) -> Result<()> {
        for record in records {
            self.apply_page_wal_record(record)?;
        }
        Ok(())
    }

    pub fn apply_page_wal_record(&mut self, record: &PageWalRecord) -> Result<()> {
        match &record.mutation {
            PageWalMutation::Put {
                cid,
                eid,
                rid,
                fields,
            } if *cid == self.cid && *eid == self.eid => {
                if self.directory.resolve(*rid).is_ok() && self.get(*rid).is_ok() {
                    return Ok(());
                }
                let borrowed: Vec<(FieldId, &[u8])> = fields
                    .iter()
                    .map(|(field, bytes)| (*field, bytes.as_slice()))
                    .collect();
                self.apply_put(*rid, &borrowed)
            }
            PageWalMutation::Delete { cid, eid, rid } if *cid == self.cid && *eid == self.eid => {
                if self.directory.resolve(*rid).is_err() || self.get(*rid).is_err() {
                    return Ok(());
                }
                self.delete(*rid)
            }
            _ => Ok(()),
        }
    }

    fn apply_put(&mut self, rid: Rid, fields: &[(FieldId, &[u8])]) -> Result<()> {
        let sid = Sid(self.next_sid);
        let slot = RecordSlot::from_fields(sid, fields);
        let address = self.write_slot(&slot)?;
        self.directory.insert(rid, address);
        self.next_rid = self.next_rid.max(rid.0.saturating_add(1));
        self.next_sid = self.next_sid.saturating_add(1).max(1);
        self.flush_directory()?;
        Ok(())
    }

    pub fn get(&self, rid: Rid) -> Result<Vec<u8>> {
        let address = self.directory.resolve(rid)?;
        let page = self.load_page(address.pid)?;
        page.get(address.sid)?
            .map(|bytes| bytes.to_vec())
            .ok_or(PraxisError::KeyNotFound)
    }

    pub fn delete(&mut self, rid: Rid) -> Result<()> {
        let address = self.directory.resolve(rid)?;
        let mut page = self.load_page(address.pid)?;
        page.delete(address.sid)?;
        self.write_page(address.pid, &page)?;
        self.write_slot_metadata(address.pid, page.slots())?;
        Ok(())
    }

    pub fn directory(&self) -> &RidDirectory {
        &self.directory
    }

    pub fn layout(&self) -> &PraxisLayout {
        &self.layout
    }

    fn write_slot(&mut self, slot: &RecordSlot) -> Result<SlotAddress> {
        let mut page = self.load_or_create_page(self.current_pid)?;
        if let Err(PraxisError::WriteStalled { .. }) = slot.install_into_page(&mut page) {
            self.current_pid = Pid(self.current_pid.0.saturating_add(1));
            self.next_sid = 1;
            let sid = Sid(self.next_sid);
            let relocated = RecordSlot {
                sid,
                body: slot.body.clone(),
                fields: slot.fields.clone(),
            };
            let mut next = RecordPage::new(self.next_rid);
            relocated.install_into_page(&mut next)?;
            self.write_page(self.current_pid, &next)?;
            self.write_slot_metadata(self.current_pid, next.slots())?;
            return Ok(SlotAddress {
                cid: self.cid,
                eid: self.eid,
                fid: self.fid,
                pid: self.current_pid,
                sid,
            });
        }
        self.write_page(self.current_pid, &page)?;
        self.write_slot_metadata(self.current_pid, page.slots())?;
        Ok(SlotAddress {
            cid: self.cid,
            eid: self.eid,
            fid: self.fid,
            pid: self.current_pid,
            sid: slot.sid,
        })
    }

    fn load_or_create_page(&self, pid: Pid) -> Result<RecordPage> {
        let path = self
            .layout
            .page_path(self.cid, self.eid, self.fid, pid)
            .path;
        if path.exists() {
            RecordPage::from_bytes(&std::fs::read(&path)?, path)
        } else {
            Ok(RecordPage::new(self.next_rid))
        }
    }

    fn load_page(&self, pid: Pid) -> Result<RecordPage> {
        let path = self
            .layout
            .page_path(self.cid, self.eid, self.fid, pid)
            .path;
        RecordPage::from_bytes(&std::fs::read(&path)?, path)
    }

    fn write_page(&self, pid: Pid, page: &RecordPage) -> Result<()> {
        self.layout
            .ensure_frame_tree(self.cid, self.eid, self.fid)?;
        let path = self
            .layout
            .page_path(self.cid, self.eid, self.fid, pid)
            .path;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, page.to_bytes())?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&tmp_path)?
            .sync_all()?;
        #[cfg(windows)]
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        std::fs::rename(&tmp_path, path)?;
        self.manifest_store().track_bytes(
            &self
                .layout
                .page_path(self.cid, self.eid, self.fid, pid)
                .path,
            "record-page",
            1,
            &page.to_bytes(),
        )?;
        Ok(())
    }

    fn write_slot_metadata(&self, pid: Pid, slots: &[SlotDirectoryEntry]) -> Result<()> {
        let root = self
            .layout
            .slot_directory_root(self.cid, self.eid, self.fid, pid);
        std::fs::create_dir_all(&root)?;
        let mut directory = BinaryWriter::new();
        directory.write_u32(slots.len() as u32);
        for slot in slots {
            directory.write_u32(slot.sid.0 as u32);
            directory.write_u32(slot.offset);
            directory.write_u32(slot.len);
            directory.write_u8(u8::from(slot.tombstone));
        }
        let slot_directory = directory.into_inner();
        let slot_directory_path = root.join("slot.directory");
        std::fs::write(&slot_directory_path, &slot_directory)?;
        self.manifest_store().track_bytes(
            &slot_directory_path,
            "slot-directory",
            1,
            &slot_directory,
        )?;
        let page_header = self.load_page(pid)?.header;
        let mut header = BinaryWriter::new();
        header.write_u32(page_header.page_size);
        header.write_u64(page_header.page_lsn);
        header.write_u32(page_header.slot_count as u32);
        header.write_u32(page_header.free_space_start);
        header.write_u32(page_header.free_space_end);
        std::fs::write(root.join("page.header"), header.into_inner())?;
        std::fs::write(root.join("fixed.map"), [])?;
        std::fs::write(root.join("dynamic.map"), [])?;
        std::fs::write(root.join("free.slots"), [])?;
        std::fs::write(root.join("null.bitmap"), [])?;
        std::fs::write(root.join("varlen.map"), [])?;
        std::fs::write(root.join("overflow.map"), [])?;
        Ok(())
    }

    fn flush_directory(&self) -> Result<()> {
        let path = self.layout.critical_map_path(self.cid, "rid_to_slot").path;
        self.directory
            .save_binary(&path, self.next_rid, self.next_sid, self.current_pid)?;
        self.manifest_store().track_bytes(
            &path,
            "rid-directory",
            RID_DIRECTORY_VERSION as u16,
            &std::fs::read(&path)?,
        )?;
        self.flush_critical_sidecar_maps()
    }

    fn encode_typed_record(&self, fields: &[(FieldId, PraxisValue)]) -> Result<Vec<u8>> {
        let mut cells = Vec::with_capacity(fields.len());
        for (field, value) in fields {
            let encoded = DataTypeRegistry::encode(value)?;
            let payload = value.payload_bytes()?;
            let cell = match encoded.storage_class {
                StorageClass::Inline => TypedFieldCell {
                    field: *field,
                    type_code: encoded.type_code,
                    storage_class: StorageClass::Inline,
                    inline_bytes: encoded.inline_bytes,
                    pointer: None,
                },
                StorageClass::External => {
                    let location = self.overflow_store()?.append(&payload)?;
                    TypedFieldCell {
                        field: *field,
                        type_code: encoded.type_code,
                        storage_class: StorageClass::External,
                        inline_bytes: Vec::new(),
                        pointer: Some(ExternalPointer::Overflow {
                            page_id: location.file_id,
                            offset: location.offset as u32,
                            len: location.len,
                            checksum: location.checksum,
                        }),
                    }
                }
                StorageClass::Segment => {
                    let family = pointer_family(encoded.pointer)?;
                    let location = self.segment_store(family)?.append(&payload)?;
                    TypedFieldCell {
                        field: *field,
                        type_code: encoded.type_code,
                        storage_class: StorageClass::Segment,
                        inline_bytes: Vec::new(),
                        pointer: Some(ExternalPointer::Segment {
                            family,
                            segment_id: location.file_id,
                            offset: location.offset,
                            len: location.len,
                        }),
                    }
                }
                StorageClass::Catalog => TypedFieldCell {
                    field: *field,
                    type_code: encoded.type_code,
                    storage_class: StorageClass::Catalog,
                    inline_bytes: Vec::new(),
                    pointer: encoded.pointer,
                },
            };
            cells.push(cell);
        }
        encode_typed_cells(&cells)
    }

    fn overflow_store(&self) -> Result<BlobStore> {
        BlobStore::open_overflow(self.layout.overflow_blob_path(self.cid, self.eid).path)
    }

    fn segment_store(&self, family: SegmentFamily) -> Result<BlobStore> {
        BlobStore::open_segment(
            self.layout
                .segment_blob_path(self.cid, self.eid, family)
                .path,
            family,
            1,
        )
    }

    fn manifest_store(&self) -> PageManifestStore {
        PageManifestStore::open(self.layout.page_manifest_path().path)
    }

    fn flush_critical_sidecar_maps(&self) -> Result<()> {
        let page_path = self.layout.critical_map_path(self.cid, "rid_to_page").path;
        let entity_path = self
            .layout
            .critical_map_path(self.cid, "rid_to_entity")
            .path;
        let commit_path = self.layout.critical_map_path(self.cid, "commit_state").path;
        let sequence_path = self.layout.critical_map_path(self.cid, "sequence").path;

        let mut rid_to_page = BinaryWriter::new();
        let mut rid_to_entity = BinaryWriter::new();
        let mut commit_state = BinaryWriter::new();
        rid_to_page.write_u32(self.directory.mappings.len() as u32);
        rid_to_entity.write_u32(self.directory.mappings.len() as u32);
        commit_state.write_u32(self.directory.mappings.len() as u32);
        for (rid, address) in &self.directory.mappings {
            rid_to_page.write_u64(rid.0);
            rid_to_page.write_u32(address.fid.0);
            rid_to_page.write_u32(address.pid.0);
            rid_to_page.write_u32(address.sid.0 as u32);

            rid_to_entity.write_u64(rid.0);
            rid_to_entity.write_u32(address.cid.0);
            rid_to_entity.write_u32(address.eid.0);

            commit_state.write_u64(rid.0);
            commit_state.write_u8(1);
        }
        let mut sequence = BinaryWriter::new();
        sequence.write_u64(self.next_rid);
        sequence.write_u32(self.next_sid as u32);
        sequence.write_u32(self.current_pid.0);

        write_sidecar(&page_path, rid_to_page.into_inner())?;
        write_sidecar(&entity_path, rid_to_entity.into_inner())?;
        write_sidecar(&commit_path, commit_state.into_inner())?;
        write_sidecar(&sequence_path, sequence.into_inner())?;
        Ok(())
    }
}

fn corrupt<T>(path: &Path, reason: &str) -> Result<T> {
    Err(PraxisError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    })
}

fn write_sidecar(path: &Path, bytes: Vec<u8>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TypedFieldCell {
    field: FieldId,
    type_code: TypeCode,
    storage_class: StorageClass,
    inline_bytes: Vec<u8>,
    pointer: Option<ExternalPointer>,
}

fn encode_typed_cells(cells: &[TypedFieldCell]) -> Result<Vec<u8>> {
    let mut payload = BinaryWriter::new();
    payload.write_u32(cells.len() as u32);
    for cell in cells {
        write_field(&mut payload, cell.field);
        payload.write_u8(cell.type_code as u8);
        payload.write_u8(storage_class_to_tag(cell.storage_class));
        payload.write_len_bytes(&cell.inline_bytes);
        write_external_pointer(&mut payload, cell.pointer.clone());
    }
    let payload = payload.into_inner();
    let mut out = BinaryWriter::new();
    out.write_u32(TYPED_RECORD_MAGIC);
    out.write_u8(TYPED_RECORD_VERSION);
    out.write_u32(payload.len() as u32);
    out.write_u32(crc32c(&payload));
    out.write_bytes(&payload);
    Ok(out.into_inner())
}

fn decode_typed_record(bytes: &[u8], source: &Path) -> Result<Vec<TypedFieldCell>> {
    if bytes.len() < 13 {
        return corrupt(source, "truncated typed record header");
    }
    if u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != TYPED_RECORD_MAGIC {
        return corrupt(source, "bad typed record magic");
    }
    if bytes[4] != TYPED_RECORD_VERSION {
        return corrupt(
            source,
            &format!("unsupported typed record version {}", bytes[4]),
        );
    }
    let len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
    let expected = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
    if bytes.len() < 13 + len {
        return corrupt(source, "truncated typed record payload");
    }
    let payload = &bytes[13..13 + len];
    let actual = crc32c(payload);
    if actual != expected {
        return corrupt(
            source,
            &format!(
                "typed record checksum mismatch: expected {expected:#010x}, got {actual:#010x}"
            ),
        );
    }
    let mut reader = BinaryReader::new(payload, source.to_path_buf());
    let count = reader.read_u32()? as usize;
    let mut cells = Vec::with_capacity(count);
    for _ in 0..count {
        cells.push(TypedFieldCell {
            field: read_field(&mut reader, source)?,
            type_code: tag_to_type_code(reader.read_u8()?, source)?,
            storage_class: tag_to_storage_class(reader.read_u8()?, source)?,
            inline_bytes: reader.read_len_bytes()?,
            pointer: read_external_pointer(&mut reader, source)?,
        });
    }
    Ok(cells)
}

fn pointer_family(pointer: Option<ExternalPointer>) -> Result<SegmentFamily> {
    match pointer {
        Some(ExternalPointer::Segment { family, .. }) => Ok(family),
        _ => Err(PraxisError::InvalidConfig(
            "segment value did not include segment family".to_string(),
        )),
    }
}

fn write_field(writer: &mut BinaryWriter, field: FieldId) {
    match field {
        FieldId::Fixed(aid) => {
            writer.write_u8(1);
            writer.write_u32(aid.0 as u32);
        }
        FieldId::Dynamic(did) => {
            writer.write_u8(2);
            writer.write_u32(did.0 as u32);
        }
    }
}

fn read_field(reader: &mut BinaryReader<'_>, source: &Path) -> Result<FieldId> {
    match reader.read_u8()? {
        1 => Ok(FieldId::Fixed(Aid(reader.read_u32()? as u16))),
        2 => Ok(FieldId::Dynamic(Did(reader.read_u32()? as u16))),
        tag => corrupt(source, &format!("invalid typed field tag {tag}")),
    }
}

fn write_external_pointer(writer: &mut BinaryWriter, pointer: Option<ExternalPointer>) {
    match pointer {
        None => writer.write_u8(0),
        Some(ExternalPointer::Overflow {
            page_id,
            offset,
            len,
            checksum,
        }) => {
            writer.write_u8(1);
            writer.write_u64(page_id);
            writer.write_u32(offset);
            writer.write_u32(len);
            writer.write_u32(checksum);
        }
        Some(ExternalPointer::Segment {
            family,
            segment_id,
            offset,
            len,
        }) => {
            writer.write_u8(2);
            writer.write_u8(segment_family_to_tag(family));
            writer.write_u64(segment_id);
            writer.write_u64(offset);
            writer.write_u32(len);
        }
        Some(ExternalPointer::Catalog { catalog, key }) => {
            writer.write_u8(3);
            writer.write_len_bytes(catalog.as_bytes());
            writer.write_u64(key);
        }
    }
}

fn read_external_pointer(
    reader: &mut BinaryReader<'_>,
    source: &Path,
) -> Result<Option<ExternalPointer>> {
    match reader.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(ExternalPointer::Overflow {
            page_id: reader.read_u64()?,
            offset: reader.read_u32()?,
            len: reader.read_u32()?,
            checksum: reader.read_u32()?,
        })),
        2 => Ok(Some(ExternalPointer::Segment {
            family: tag_to_segment_family(reader.read_u8()?, source)?,
            segment_id: reader.read_u64()?,
            offset: reader.read_u64()?,
            len: reader.read_u32()?,
        })),
        3 => {
            let catalog = String::from_utf8(reader.read_len_bytes()?)
                .map_err(|err| PraxisError::InvalidConfig(err.to_string()))?;
            Ok(Some(ExternalPointer::Catalog {
                catalog,
                key: reader.read_u64()?,
            }))
        }
        tag => corrupt(source, &format!("invalid typed pointer tag {tag}")),
    }
}

fn storage_class_to_tag(storage_class: StorageClass) -> u8 {
    match storage_class {
        StorageClass::Inline => 1,
        StorageClass::External => 2,
        StorageClass::Segment => 3,
        StorageClass::Catalog => 4,
    }
}

fn tag_to_storage_class(tag: u8, source: &Path) -> Result<StorageClass> {
    match tag {
        1 => Ok(StorageClass::Inline),
        2 => Ok(StorageClass::External),
        3 => Ok(StorageClass::Segment),
        4 => Ok(StorageClass::Catalog),
        _ => corrupt(source, &format!("invalid storage class tag {tag}")),
    }
}

fn segment_family_to_tag(family: SegmentFamily) -> u8 {
    match family {
        SegmentFamily::Vector => 1,
        SegmentFamily::Edge => 2,
        SegmentFamily::FullText => 3,
    }
}

fn tag_to_segment_family(tag: u8, source: &Path) -> Result<SegmentFamily> {
    match tag {
        1 => Ok(SegmentFamily::Vector),
        2 => Ok(SegmentFamily::Edge),
        3 => Ok(SegmentFamily::FullText),
        _ => corrupt(source, &format!("invalid segment family tag {tag}")),
    }
}

fn tag_to_type_code(tag: u8, source: &Path) -> Result<TypeCode> {
    match tag {
        0x01 => Ok(TypeCode::Int2),
        0x02 => Ok(TypeCode::Int4),
        0x03 => Ok(TypeCode::Int8),
        0x04 => Ok(TypeCode::UInt8),
        0x10 => Ok(TypeCode::Float4),
        0x11 => Ok(TypeCode::Float8),
        0x12 => Ok(TypeCode::Numeric),
        0x20 => Ok(TypeCode::Bool),
        0x30 => Ok(TypeCode::TextInline),
        0x31 => Ok(TypeCode::Json),
        0x32 => Ok(TypeCode::Jsonb),
        0x40 => Ok(TypeCode::Uuid),
        0x50 => Ok(TypeCode::Date),
        0x51 => Ok(TypeCode::Time),
        0x53 => Ok(TypeCode::Timestamp),
        0x60 => Ok(TypeCode::Money),
        0x61 => Ok(TypeCode::Enum),
        0x70 => Ok(TypeCode::Bytea),
        0x80 => Ok(TypeCode::List),
        0x81 => Ok(TypeCode::Set),
        0x82 => Ok(TypeCode::Dict),
        0x83 => Ok(TypeCode::Range),
        0x90 => Ok(TypeCode::Vec32),
        0x91 => Ok(TypeCode::Vec16),
        0x92 => Ok(TypeCode::Vec8),
        0x93 => Ok(TypeCode::VecBin),
        0x94 => Ok(TypeCode::Embedding),
        0x95 => Ok(TypeCode::SparseVec),
        0xA0 => Ok(TypeCode::EdgeRef),
        0xA1 => Ok(TypeCode::EdgeList),
        0xA2 => Ok(TypeCode::Path),
        0xA3 => Ok(TypeCode::Subgraph),
        0xB0 => Ok(TypeCode::TsVector),
        0xC0 => Ok(TypeCode::GeoPoint),
        0xC1 => Ok(TypeCode::GeoShape),
        0xD0 => Ok(TypeCode::Cidr),
        0xD1 => Ok(TypeCode::Inet),
        0xE0 => Ok(TypeCode::CommitRef),
        0xE1 => Ok(TypeCode::BranchRef),
        0xE2 => Ok(TypeCode::RidRef),
        _ => corrupt(source, &format!("invalid type code tag {tag:#04x}")),
    }
}
