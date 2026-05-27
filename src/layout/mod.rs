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
pub struct PraxisLayout {
    root: PathBuf,
}

impl PraxisLayout {
    pub const ROOT_DIR: &'static str = ".praxis";
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
            self.runtime_root().join("sockets"),
            self.runtime_root().join("locks"),
            self.runtime_root().join("sessions"),
        ] {
            std::fs::create_dir_all(path)?;
        }
        for path in [
            self.config_root().join("engine.toml"),
            self.config_root().join("cache.toml"),
            self.config_root().join("compaction.toml"),
            self.config_root().join("wal.toml"),
            self.config_root().join("tiers.toml"),
            self.runtime_root().join("pid"),
            self.logs_root().join("engine.log"),
            self.logs_root().join("wal.log"),
            self.logs_root().join("compaction.log"),
            self.logs_root().join("query.log"),
            self.logs_root().join("recovery.log"),
            self.catalog_root().join("collections.cat"),
            self.catalog_root().join("entities.cat"),
            self.catalog_root().join("indexes.cat"),
            self.catalog_root().join("vector_models.cat"),
            self.catalog_root().join("fulltext.cat"),
            self.catalog_root().join("graph.cat"),
            self.catalog_root().join("shards.cat"),
            self.catalog_root().join("users.cat"),
        ] {
            ensure_empty_file(&path)?;
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

    pub fn collection_meta_root(&self, cid: Cid) -> PathBuf {
        self.collection_root(cid).join("meta")
    }

    pub fn collection_wal_root(&self, cid: Cid) -> PathBuf {
        self.collection_root(cid).join("wal")
    }

    pub fn collection_active_wal_path(&self, cid: Cid, wal_id: u64) -> LayoutPath {
        self.layout_path(
            DurableLayoutKind::Logs,
            self.collection_wal_root(cid)
                .join("active")
                .join(format!("{wal_id:08}.wal")),
        )
    }

    pub fn collection_archived_wal_path(&self, cid: Cid, wal_id: u64) -> LayoutPath {
        self.layout_path(
            DurableLayoutKind::Logs,
            self.collection_wal_root(cid)
                .join("archived")
                .join(format!("{wal_id:08}.wal")),
        )
    }

    pub fn collection_kv_root(&self, cid: Cid) -> PathBuf {
        self.collection_root(cid).join("kv")
    }

    pub fn entity_root(&self, cid: Cid, eid: Eid) -> PathBuf {
        self.collection_root(cid)
            .join("entities")
            .join(eid.to_hex())
    }

    pub fn entity_meta_path(&self, cid: Cid, eid: Eid) -> LayoutPath {
        self.layout_path(
            DurableLayoutKind::Entity,
            self.entity_root(cid, eid).join("entity.meta"),
        )
    }

    pub fn entity_layout_path(&self, cid: Cid, eid: Eid) -> LayoutPath {
        self.layout_path(
            DurableLayoutKind::Entity,
            self.entity_root(cid, eid).join("layout.bin"),
        )
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

    pub fn frame_root(&self, cid: Cid, eid: Eid, fid: Fid) -> PathBuf {
        self.entity_root(cid, eid)
            .join("pages")
            .join("frames")
            .join(fid.to_hex())
    }

    pub fn ensure_frame_tree(&self, cid: Cid, eid: Eid, fid: Fid) -> Result<()> {
        let frame = self.frame_root(cid, eid, fid);
        std::fs::create_dir_all(frame.join("pages"))?;
        for path in [
            frame.join("frame.meta"),
            frame.join("frame.stats"),
            frame.join("frame.idx"),
            frame.join("frame.bloom"),
            frame.join("frame.minmax"),
            frame.join("frame.free"),
        ] {
            ensure_empty_file(&path)?;
        }
        Ok(())
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
                .join("0001")
                .join("0001")
                .join("values")
                .join("000001.ovf"),
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
        self.ensure_collection_tree(cid)?;
        let entity = self.entity_root(cid, eid);
        for path in [
            entity.clone(),
            entity.join("schema").join("sql"),
            entity.join("schema").join("sql").join("fixed_attrs"),
            entity.join("schema").join("nosql"),
            entity.join("schema").join("nosql").join("dynamic_attrs"),
            entity.join("pages").join("frames"),
            entity.join("slots"),
            entity
                .join("overflow")
                .join("0001")
                .join("0001")
                .join("values"),
            entity.join("overflow"),
            entity.join("indexes").join("primary"),
            entity.join("indexes").join("primary").join("btree"),
            entity.join("indexes").join("primary").join("hash"),
            entity.join("indexes").join("secondary"),
            entity.join("indexes").join("secondary").join("range"),
            entity.join("indexes").join("secondary").join("bitmap"),
            entity.join("indexes").join("secondary").join("hash"),
            entity.join("indexes").join("secondary").join("composite"),
            entity.join("indexes").join("secondary").join("unique"),
            entity.join("indexes").join("text"),
            entity.join("indexes").join("vector"),
            entity.join("indexes").join("vector").join("embeddings"),
            entity.join("indexes").join("graph"),
            entity.join("vectors"),
            entity.join("vectors").join("models"),
            entity.join("vectors").join("pages"),
            entity.join("vectors").join("sparse"),
            entity.join("vectors").join("quantized"),
            entity.join("fulltext"),
            entity.join("fulltext").join("pages"),
            entity.join("fulltext").join("postings"),
            entity.join("graph"),
            entity.join("graph").join("nodes"),
            entity.join("graph").join("relationships"),
            entity.join("graph").join("adjacency"),
            entity.join("graph").join("traversal"),
            entity.join("graph").join("subgraphs"),
            entity.join("graph").join("paths"),
            entity.join("history"),
            entity.join("history").join("commits"),
            entity.join("history").join("snapshots"),
            entity.join("history").join("branches"),
            entity.join("history").join("deltas"),
            entity.join("history").join("timelines"),
            entity.join("history").join("gc"),
            entity.join("compaction"),
            entity.join("cache"),
            entity.join("cache").join("hot"),
            entity.join("cache").join("warm"),
            entity.join("cache").join("cold"),
            entity.join("cache").join("bloom"),
            entity.join("cache").join("query"),
            entity.join("cache").join("vectors"),
            entity.join("analytics"),
            entity.join("analytics").join("materialized"),
            entity.join("analytics").join("rollups"),
            entity.join("analytics").join("aggregates"),
            entity.join("analytics").join("timeseries"),
            entity.join("analytics").join("summaries"),
        ] {
            std::fs::create_dir_all(path)?;
        }
        for path in [
            entity.join("entity.meta"),
            entity.join("entity.stats"),
            entity.join("layout.bin"),
            entity.join("rid.map"),
            entity.join("page.map"),
            entity.join("slot.map"),
            entity.join("schema").join("schema.meta"),
            entity.join("schema").join("schema.version"),
            entity.join("schema").join("sql").join("columns.cat"),
            entity.join("schema").join("sql").join("constraints.cat"),
            entity.join("schema").join("sql").join("defaults.cat"),
            entity.join("schema").join("sql").join("types.cat"),
            entity.join("schema").join("sql").join("nullable.bitmap"),
            entity.join("schema").join("sql").join("layout.idx"),
            entity.join("schema").join("nosql").join("fields.cat"),
            entity.join("schema").join("nosql").join("dynamic_keys.cat"),
            entity.join("schema").join("nosql").join("codecs.cat"),
            entity
                .join("schema")
                .join("nosql")
                .join("inferred_types.cat"),
            entity.join("schema").join("nosql").join("validators.cat"),
            entity.join("pages").join("page.catalog"),
            entity.join("pages").join("free.pages"),
            entity.join("pages").join("dirty.pages"),
            entity.join("pages").join("hot.pages"),
            entity.join("indexes").join("primary").join("primary.idx"),
            entity.join("indexes").join("primary").join("primary.btree"),
            entity.join("indexes").join("primary").join("primary.hash"),
            entity.join("indexes").join("text").join("lexicon.idx"),
            entity.join("indexes").join("text").join("gin.idx"),
            entity.join("indexes").join("text").join("bm25.idx"),
            entity.join("indexes").join("text").join("tokenizer.meta"),
            entity.join("indexes").join("vector").join("hnsw.idx"),
            entity.join("indexes").join("vector").join("ivf.idx"),
            entity.join("indexes").join("vector").join("pq.codebook"),
            entity.join("indexes").join("vector").join("quant.meta"),
            entity.join("indexes").join("graph").join("edge_out.idx"),
            entity.join("indexes").join("graph").join("edge_in.idx"),
            entity.join("indexes").join("graph").join("degree.idx"),
            entity.join("indexes").join("graph").join("traversal.idx"),
            entity.join("indexes").join("graph").join("path.idx"),
            entity.join("vectors").join("vector.meta"),
            entity.join("fulltext").join("fts.meta"),
            entity.join("fulltext").join("tokenizer.meta"),
            entity.join("fulltext").join("stopwords.txt"),
            entity.join("compaction").join("compact.plan"),
            entity.join("compaction").join("relocation.log"),
            entity.join("compaction").join("forwarding.idx"),
            entity.join("compaction").join("tombstones.idx"),
            entity.join("compaction").join("vacuum.queue"),
            entity.join("compaction").join("rewrite.map"),
        ] {
            ensure_empty_file(&path)?;
        }
        Ok(())
    }

    pub fn ensure_collection_tree(&self, cid: Cid) -> Result<()> {
        let collection = self.collection_root(cid);
        for path in [
            collection.join("meta"),
            collection.join("wal").join("active"),
            collection.join("wal").join("archived"),
            collection.join("kv").join("levels").join("L0"),
            collection.join("kv").join("levels").join("L1"),
            collection.join("kv").join("levels").join("L2"),
            collection.join("kv").join("levels").join("Ln"),
            collection.join("kv").join("critical"),
            collection.join("kv").join("secondary"),
            collection.join("kv").join("cache"),
            collection.join("entities"),
            collection.join("pit").join("commits"),
            collection.join("pit").join("branches"),
            collection.join("pit").join("snapshots"),
            collection.join("pit").join("timelines"),
            collection.join("pit").join("gc"),
            collection.join("pit").join("packfiles"),
            collection.join("pit").join("recovery"),
            collection.join("tiers").join("hot"),
            collection.join("tiers").join("warm"),
            collection.join("tiers").join("cold"),
            collection.join("ops").join("transactions"),
            collection.join("ops").join("checkpoints"),
            collection.join("ops").join("recovery"),
            collection.join("ops").join("metrics"),
            collection.join("ops").join("planner"),
            collection.join("ops").join("scheduler"),
            collection.join("ops").join("telemetry"),
            collection.join("tmp").join("compaction"),
            collection.join("tmp").join("restore"),
            collection.join("tmp").join("import"),
            collection.join("tmp").join("rebuild"),
        ] {
            std::fs::create_dir_all(path)?;
        }
        for path in [
            collection.join("meta").join("collection.meta"),
            collection.join("meta").join("collection.stats"),
            collection.join("meta").join("collection.config"),
            collection.join("meta").join("retention.policy"),
            collection.join("wal").join("wal.meta"),
            collection.join("wal").join("checkpoint"),
            collection.join("kv").join("manifest"),
            collection.join("kv").join("memtable.wal"),
            collection
                .join("kv")
                .join("critical")
                .join("rid_to_slot.kv"),
            collection
                .join("kv")
                .join("critical")
                .join("rid_to_page.kv"),
            collection
                .join("kv")
                .join("critical")
                .join("rid_to_entity.kv"),
            collection.join("kv").join("critical").join("locks.kv"),
            collection
                .join("kv")
                .join("critical")
                .join("commit_state.kv"),
            collection.join("kv").join("critical").join("sequence.kv"),
            collection
                .join("kv")
                .join("secondary")
                .join("value_to_rid.kv"),
            collection.join("kv").join("secondary").join("edge_out.kv"),
            collection.join("kv").join("secondary").join("edge_in.kv"),
            collection
                .join("kv")
                .join("secondary")
                .join("text_lookup.kv"),
            collection
                .join("kv")
                .join("secondary")
                .join("vector_lookup.kv"),
            collection
                .join("kv")
                .join("secondary")
                .join("time_to_commit.kv"),
            collection.join("kv").join("cache").join("bloom.filters"),
            collection.join("kv").join("cache").join("hot.keys"),
            collection.join("kv").join("cache").join("key.stats"),
        ] {
            ensure_empty_file(&path)?;
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

fn ensure_empty_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        std::fs::write(path, [])?;
    }
    Ok(())
}
