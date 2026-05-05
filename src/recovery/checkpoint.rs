use crate::errors::Result;
use crate::io::file_digest;
use crate::manifest::{CheckpointMetadata, Manifest};
use serde::{Deserialize, Serialize};
use std::fs;
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
    fs::write(&tmp, serde_json::to_vec_pretty(&checkpoint)?)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp, path)?;
    let digest = file_digest(path)?;
    Ok(CheckpointMetadata {
        id,
        path: path.to_string_lossy().to_string(),
        sequence: manifest.last_sequence,
        segment_count: manifest.segments.len() as u64,
        file_digest: digest.to_hex().to_string(),
    })
}
