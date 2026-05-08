use crate::errors::{Result, TridentError};
use crate::identity::{Aid, Cid, Did, Eid, Fid, FieldId, Pid, Rid, Sid, SlotAddress, Vid};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use crate::layout::TridentLayout;
use crate::page::{RecordPage, SlotDirectoryEntry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

const RID_DIRECTORY_MAGIC: u32 = 0x5452_4944;
const RID_DIRECTORY_VERSION: u8 = 1;

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
    layout: TridentLayout,
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
            return Err(TridentError::Corrupt {
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
            .ok_or(TridentError::KeyNotFound)
    }

    pub fn relocate(&mut self, rid: Rid, address: SlotAddress) -> Result<()> {
        if !self.mappings.contains_key(&rid) {
            return Err(TridentError::KeyNotFound);
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
        let layout = TridentLayout::new(base);
        layout.create_all()?;
        layout.ensure_entity_tree(cid, eid)?;
        let map_path = layout.critical_map_path(cid, "rid_to_slot").path;
        let (directory, next_rid, next_sid, current_pid) = if map_path.exists() {
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
        let sid = Sid(self.next_sid);
        let slot = RecordSlot::from_fields(sid, fields);
        let address = self.write_slot(&slot)?;
        self.directory.insert(rid, address);
        self.next_rid = self.next_rid.saturating_add(1);
        self.next_sid = self.next_sid.saturating_add(1).max(1);
        self.flush_directory()?;
        Ok(rid)
    }

    pub fn get(&self, rid: Rid) -> Result<Vec<u8>> {
        let address = self.directory.resolve(rid)?;
        let page = self.load_page(address.pid)?;
        page.get(address.sid)?
            .map(|bytes| bytes.to_vec())
            .ok_or(TridentError::KeyNotFound)
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

    pub fn layout(&self) -> &TridentLayout {
        &self.layout
    }

    fn write_slot(&mut self, slot: &RecordSlot) -> Result<SlotAddress> {
        let mut page = self.load_or_create_page(self.current_pid)?;
        if let Err(TridentError::WriteStalled { .. }) = slot.install_into_page(&mut page) {
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
        std::fs::write(root.join("slot.directory"), directory.into_inner())?;
        std::fs::write(root.join("free.slots"), [])?;
        std::fs::write(root.join("null.bitmap"), [])?;
        std::fs::write(root.join("varlen.map"), [])?;
        std::fs::write(root.join("overflow.map"), [])?;
        Ok(())
    }

    fn flush_directory(&self) -> Result<()> {
        let path = self.layout.critical_map_path(self.cid, "rid_to_slot").path;
        self.directory
            .save_binary(&path, self.next_rid, self.next_sid, self.current_pid)
    }
}

fn corrupt<T>(path: &Path, reason: &str) -> Result<T> {
    Err(TridentError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    })
}
