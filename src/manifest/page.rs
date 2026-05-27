use crate::errors::{PraxisError, Result};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use crate::manifest::ManifestTrackedFile;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

const PAGE_MANIFEST_MAGIC: u32 = 0x5450_4d46;
const PAGE_MANIFEST_VERSION: u8 = 1;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PageLayoutManifest {
    pub files: Vec<ManifestTrackedFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageManifestStore {
    path: PathBuf,
}

impl PageLayoutManifest {
    pub fn upsert_file(&mut self, file: ManifestTrackedFile) {
        if let Some(existing) = self
            .files
            .iter_mut()
            .find(|existing| existing.path == file.path)
        {
            *existing = file;
        } else {
            self.files.push(file);
            self.files.sort_by(|left, right| left.path.cmp(&right.path));
        }
    }
}

impl PageManifestStore {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> Result<PageLayoutManifest> {
        if !self.path.exists() {
            return Ok(PageLayoutManifest::default());
        }
        let bytes = std::fs::read(&self.path)?;
        if bytes.len() < 13 {
            return corrupt(&self.path, "truncated page manifest header");
        }
        if u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) != PAGE_MANIFEST_MAGIC {
            return corrupt(&self.path, "bad page manifest magic");
        }
        if bytes[4] != PAGE_MANIFEST_VERSION {
            return corrupt(
                &self.path,
                &format!("unsupported page manifest version {}", bytes[4]),
            );
        }
        let len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
        let expected = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
        if bytes.len() < 13 + len {
            return corrupt(&self.path, "truncated page manifest payload");
        }
        let payload = &bytes[13..13 + len];
        let actual = crc32c(payload);
        if actual != expected {
            return corrupt(
                &self.path,
                &format!(
                    "page manifest checksum mismatch: expected {expected:#010x}, got {actual:#010x}"
                ),
            );
        }
        decode_manifest(payload, &self.path)
    }

    pub fn save(&self, manifest: &PageLayoutManifest) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let payload = encode_manifest(manifest);
        let mut out = BinaryWriter::new();
        out.write_u32(PAGE_MANIFEST_MAGIC);
        out.write_u8(PAGE_MANIFEST_VERSION);
        out.write_u32(payload.len() as u32);
        out.write_u32(crc32c(&payload));
        out.write_bytes(&payload);
        let tmp_path = self.path.with_extension("tmp");
        std::fs::write(&tmp_path, out.into_inner())?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&tmp_path)?
            .sync_all()?;
        #[cfg(windows)]
        if self.path.exists() {
            std::fs::remove_file(&self.path)?;
        }
        std::fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }

    pub fn track_file(&self, file: ManifestTrackedFile) -> Result<()> {
        let mut manifest = self.load()?;
        manifest.upsert_file(file);
        self.save(&manifest)
    }

    pub fn track_bytes(
        &self,
        path: &Path,
        format: &str,
        format_version: u16,
        bytes: &[u8],
    ) -> Result<()> {
        self.track_file(ManifestTrackedFile::durable(
            path.to_string_lossy(),
            format,
            format_version,
            format!("{:08x}", crc32c(bytes)),
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn encode_manifest(manifest: &PageLayoutManifest) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u32(manifest.files.len() as u32);
    for file in &manifest.files {
        writer.write_len_bytes(file.path.as_bytes());
        writer.write_len_bytes(file.format.as_bytes());
        writer.write_u32(file.format_version as u32);
        writer.write_len_bytes(file.checksum.as_bytes());
        writer.write_u8(u8::from(file.recoverable));
    }
    writer.into_inner()
}

fn decode_manifest(bytes: &[u8], path: &Path) -> Result<PageLayoutManifest> {
    let mut reader = BinaryReader::new(bytes, path.to_path_buf());
    let count = reader.read_u32()? as usize;
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
        files.push(ManifestTrackedFile {
            path: read_string(&mut reader)?,
            format: read_string(&mut reader)?,
            format_version: reader.read_u32()? as u16,
            checksum: read_string(&mut reader)?,
            recoverable: reader.read_u8()? != 0,
        });
    }
    Ok(PageLayoutManifest { files })
}

fn read_string(reader: &mut BinaryReader<'_>) -> Result<String> {
    String::from_utf8(reader.read_len_bytes()?)
        .map_err(|err| PraxisError::InvalidConfig(err.to_string()))
}

fn corrupt<T>(path: &Path, reason: &str) -> Result<T> {
    Err(PraxisError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    })
}
