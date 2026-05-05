use crate::errors::Result;
use crate::io::file_digest;
use crate::manifest::{CheckpointMetadata, Manifest};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CheckpointFile {
    pub format_version: u32,
    pub sequence: u64,
    pub segment_paths: Vec<String>,
}

pub fn write_checkpoint(path: &Path, id: u64, manifest: &Manifest) -> Result<CheckpointMetadata> {
    let checkpoint = CheckpointFile {
        format_version: manifest.format_version,
        sequence: manifest.last_sequence,
        segment_paths: manifest
            .segments
            .iter()
            .map(|segment| segment.path.clone())
            .collect(),
    };
    let tmp = path.with_extension("tmp");
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(&checkpoint)?)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    sync_parent_dir(path)?;
    let digest = file_digest(path)?;
    Ok(CheckpointMetadata {
        id,
        path: path.to_string_lossy().to_string(),
        sequence: manifest.last_sequence,
        segment_count: manifest.segments.len() as u64,
        file_digest: digest.to_hex().to_string(),
    })
}

#[cfg(unix)]
fn sync_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_dir(_path: &Path) -> Result<()> {
    Ok(())
}
