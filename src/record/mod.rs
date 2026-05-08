use crate::errors::{Result, TridentError};
use crate::identity::{Aid, Did, FieldId, Rid, Sid, SlotAddress, Vid};
use crate::page::RecordPage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
}
