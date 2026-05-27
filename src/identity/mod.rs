use crate::errors::{PraxisError, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($name:ident, $raw:ty, $width:literal) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            Eq,
            PartialEq,
            Ord,
            PartialOrd,
            Hash,
            Serialize,
            Deserialize,
        )]
        pub struct $name(pub $raw);

        impl $name {
            pub const NULL: Self = Self(0);

            pub fn is_null(self) -> bool {
                self.0 == 0
            }

            pub fn to_hex(self) -> String {
                format!("{:0width$x}", self.0, width = $width)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{:0width$x}", self.0, width = $width)
            }
        }
    };
}

id_type!(Cid, u32, 4);
id_type!(Eid, u32, 4);
id_type!(Fid, u32, 4);
id_type!(Pid, u32, 4);
id_type!(Sid, u16, 4);
id_type!(Rid, u64, 16);
id_type!(Aid, u16, 4);
id_type!(Did, u16, 4);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct SlotAddress {
    pub cid: Cid,
    pub eid: Eid,
    pub fid: Fid,
    pub pid: Pid,
    pub sid: Sid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum FieldId {
    Fixed(Aid),
    Dynamic(Did),
}

impl FieldId {
    pub fn raw(self) -> u16 {
        match self {
            Self::Fixed(aid) => aid.0,
            Self::Dynamic(did) => did.0,
        }
    }

    pub fn is_fixed(self) -> bool {
        matches!(self, Self::Fixed(_))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Vid {
    pub eid: Eid,
    pub fid: Fid,
    pub pid: Pid,
    pub sid: Sid,
    pub field: FieldId,
}

impl Vid {
    pub fn sql(eid: Eid, fid: Fid, pid: Pid, sid: Sid, aid: Aid) -> Self {
        Self {
            eid,
            fid,
            pid,
            sid,
            field: FieldId::Fixed(aid),
        }
    }

    pub fn nosql(eid: Eid, fid: Fid, pid: Pid, sid: Sid, did: Did) -> Self {
        Self {
            eid,
            fid,
            pid,
            sid,
            field: FieldId::Dynamic(did),
        }
    }

    pub fn to_global_hex(self) -> String {
        format!(
            "{}-{}-{}-{}-{:04x}",
            self.eid,
            self.fid,
            self.pid,
            self.sid,
            self.field.raw()
        )
    }

    pub fn from_slot(address: SlotAddress, field: FieldId) -> Self {
        Self {
            eid: address.eid,
            fid: address.fid,
            pid: address.pid,
            sid: address.sid,
            field,
        }
    }

    pub fn to_entity_relative(self) -> RelativeVid {
        RelativeVid::EntityRelative {
            fid: self.fid,
            pid: self.pid,
            sid: self.sid,
            field: self.field,
        }
    }

    pub fn to_frame_relative(self) -> RelativeVid {
        RelativeVid::FrameRelative {
            pid: self.pid,
            sid: self.sid,
            field: self.field,
        }
    }

    pub fn to_page_relative(self) -> RelativeVid {
        RelativeVid::PageRelative {
            sid: self.sid,
            field: self.field,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct VidContext {
    pub eid: Option<Eid>,
    pub fid: Option<Fid>,
    pub pid: Option<Pid>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RelativeVid {
    Global(Vid),
    EntityRelative {
        fid: Fid,
        pid: Pid,
        sid: Sid,
        field: FieldId,
    },
    FrameRelative {
        pid: Pid,
        sid: Sid,
        field: FieldId,
    },
    PageRelative {
        sid: Sid,
        field: FieldId,
    },
}

impl RelativeVid {
    pub fn resolve(self, context: VidContext) -> Result<Vid> {
        match self {
            Self::Global(vid) => Ok(vid),
            Self::EntityRelative {
                fid,
                pid,
                sid,
                field,
            } => Ok(Vid {
                eid: required(context.eid, "EID")?,
                fid,
                pid,
                sid,
                field,
            }),
            Self::FrameRelative { pid, sid, field } => Ok(Vid {
                eid: required(context.eid, "EID")?,
                fid: required(context.fid, "FID")?,
                pid,
                sid,
                field,
            }),
            Self::PageRelative { sid, field } => Ok(Vid {
                eid: required(context.eid, "EID")?,
                fid: required(context.fid, "FID")?,
                pid: required(context.pid, "PID")?,
                sid,
                field,
            }),
        }
    }
}

fn required<T: Copy>(value: Option<T>, name: &str) -> Result<T> {
    value.ok_or_else(|| {
        PraxisError::InvalidConfig(format!(
            "relative VID cannot be resolved without {name} context"
        ))
    })
}
