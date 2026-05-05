use crate::errors::Result;
use crate::manifest::model::Manifest;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ManifestStore {
    path: PathBuf,
}

impl ManifestStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load_or_create(&self) -> Result<Manifest> {
        if self.path.exists() {
            let bytes = fs::read_to_string(&self.path)?;
            Ok(toml::from_str(&bytes)?)
        } else {
            let manifest = Manifest::fresh();
            self.save(&manifest)?;
            Ok(manifest)
        }
    }

    pub fn save(&self, manifest: &Manifest) -> Result<()> {
        let tmp = self.path.with_extension("tmp");
        fs::write(&tmp, toml::to_string_pretty(manifest)?)?;
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        fs::rename(tmp, &self.path)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
