use crate::datatype::SegmentFamily;
use crate::errors::Result;
use crate::identity::{Cid, Eid, Fid, Pid};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DurableLayoutKind {
    Catalog,
    Config,
    Runtime,
    Logs,
    Collection,
    Entity,
    Page,
    SlotDirectory,
    CriticalMap,
    Index,
    Segment,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LayoutPath {
    pub kind: DurableLayoutKind,
    pub path: PathBuf,
    pub version: u32,
    pub manifest_tracked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TridentLayout {
    root: PathBuf,
}

impl TridentLayout {
    pub const ROOT_DIR: &'static str = ".trident";
    pub const FORMAT_VERSION: u32 = 1;

    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self {
            root: base.into().join(Self::ROOT_DIR),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn create_all(&self) -> Result<()> {
        for path in [
            self.catalog_root(),
            self.config_root(),
            self.runtime_root(),
            self.logs_root(),
            self.data_root(),
        ] {
            std::fs::create_dir_all(path)?;
        }
        Ok(())
    }

    pub fn catalog_root(&self) -> PathBuf {
        self.root.join("catalog")
    }

    pub fn config_root(&self) -> PathBuf {
        self.root.join("config")
    }

    pub fn runtime_root(&self) -> PathBuf {
        self.root.join("runtime")
    }

    pub fn logs_root(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn data_root(&self) -> PathBuf {
        self.root.join("data")
    }

    pub fn page_manifest_path(&self) -> LayoutPath {
        self.layout_path(
            DurableLayoutKind::Catalog,
            self.catalog_root().join("page-layout.manifest"),
        )
    }

    pub fn collection_root(&self, cid: Cid) -> PathBuf {
        self.data_root().join(cid.to_hex())
    }

    pub fn entity_root(&self, cid: Cid, eid: Eid) -> PathBuf {
        self.collection_root(cid)
            .join("entities")
            .join(eid.to_hex())
    }

    pub fn page_path(&self, cid: Cid, eid: Eid, fid: Fid, pid: Pid) -> LayoutPath {
        self.layout_path(
            DurableLayoutKind::Page,
            self.entity_root(cid, eid)
                .join("pages")
                .join("frames")
                .join(fid.to_hex())
                .join("pages")
                .join(format!("{}.page", pid.to_hex())),
        )
    }

    pub fn slot_directory_root(&self, cid: Cid, eid: Eid, fid: Fid, pid: Pid) -> PathBuf {
        self.entity_root(cid, eid)
            .join("slots")
            .join(fid.to_hex())
            .join(pid.to_hex())
    }

    pub fn slot_directory_path(&self, cid: Cid, eid: Eid, fid: Fid, pid: Pid) -> LayoutPath {
        self.layout_path(
            DurableLayoutKind::SlotDirectory,
            self.slot_directory_root(cid, eid, fid, pid)
                .join("slot.directory"),
        )
    }

    pub fn critical_map_path(&self, cid: Cid, name: &str) -> LayoutPath {
        self.layout_path(
            DurableLayoutKind::CriticalMap,
            self.collection_root(cid)
                .join("kv")
                .join("critical")
                .join(format!("{name}.kv")),
        )
    }

    pub fn overflow_blob_path(&self, cid: Cid, eid: Eid) -> LayoutPath {
        self.layout_path(
            DurableLayoutKind::Segment,
            self.entity_root(cid, eid)
                .join("overflow")
                .join("overflow-0001.blob"),
        )
    }

    pub fn segment_blob_path(&self, cid: Cid, eid: Eid, family: SegmentFamily) -> LayoutPath {
        let (dir, file) = match family {
            SegmentFamily::Vector => ("vectors", "vector-0001.seg"),
            SegmentFamily::Edge => ("graph", "edge-0001.seg"),
            SegmentFamily::FullText => ("fulltext", "fulltext-0001.seg"),
        };
        self.layout_path(
            DurableLayoutKind::Segment,
            self.entity_root(cid, eid).join(dir).join(file),
        )
    }

    pub fn ensure_entity_tree(&self, cid: Cid, eid: Eid) -> Result<()> {
        let entity = self.entity_root(cid, eid);
        for path in [
            entity.join("schema").join("sql"),
            entity.join("schema").join("nosql"),
            entity.join("pages").join("frames"),
            entity.join("slots"),
            entity.join("overflow"),
            entity.join("indexes").join("primary"),
            entity.join("indexes").join("secondary"),
            entity.join("indexes").join("text"),
            entity.join("indexes").join("vector"),
            entity.join("indexes").join("graph"),
            entity.join("vectors"),
            entity.join("fulltext"),
            entity.join("graph"),
            entity.join("history"),
            entity.join("compaction"),
            entity.join("cache"),
            entity.join("analytics"),
        ] {
            std::fs::create_dir_all(path)?;
        }
        Ok(())
    }

    fn layout_path(&self, kind: DurableLayoutKind, path: PathBuf) -> LayoutPath {
        LayoutPath {
            kind,
            path,
            version: Self::FORMAT_VERSION,
            manifest_tracked: true,
        }
    }
}
