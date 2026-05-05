use crate::config::TridentConfig;
use crate::errors::Result;
use crate::manifest::model::Manifest;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ManifestStore {
    path: PathBuf,
}

impl ManifestStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load_or_create(&self, config: &TridentConfig) -> Result<Manifest> {
        if self.path.exists() {
            let bytes = fs::read_to_string(&self.path)?;
            Ok(toml::from_str(&bytes)?)
        } else {
            let manifest = Manifest::fresh(config);
            self.save(&manifest)?;
            Ok(manifest)
        }
    }

    pub fn save(&self, manifest: &Manifest) -> Result<()> {
        let tmp = self.path.with_extension("tmp");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)?;
            file.write_all(toml::to_string_pretty(manifest)?.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(tmp, &self.path)?;
        sync_parent_dir(&self.path)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
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
