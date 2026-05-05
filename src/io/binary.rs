use crate::errors::{Result, TridentError};
use std::io::{Cursor, Read};
use std::path::PathBuf;

pub struct BinaryWriter {
    bytes: Vec<u8>,
}

impl BinaryWriter {
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn write_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn write_len_bytes(&mut self, bytes: &[u8]) {
        self.write_u32(bytes.len() as u32);
        self.write_bytes(bytes);
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl Default for BinaryWriter {
    fn default() -> Self {
        Self::new()
    }
}

pub struct BinaryReader<'a> {
    cursor: Cursor<&'a [u8]>,
    source: PathBuf,
}

impl<'a> BinaryReader<'a> {
    pub fn new(bytes: &'a [u8], source: impl Into<PathBuf>) -> Self {
        Self {
            cursor: Cursor::new(bytes),
            source: source.into(),
        }
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        let mut buf = [0; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0; 4];
        self.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    pub fn read_u64(&mut self) -> Result<u64> {
        let mut buf = [0; 8];
        self.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    pub fn read_len_bytes(&mut self) -> Result<Vec<u8>> {
        let len = self.read_u32()? as usize;
        let mut bytes = vec![0; len];
        self.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        self.cursor
            .read_exact(buf)
            .map_err(|_| TridentError::Corrupt {
                path: self.source.clone(),
                reason: "unexpected end of data".to_string(),
            })
    }
}
