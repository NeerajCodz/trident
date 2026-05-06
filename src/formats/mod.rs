//! Standardized binary format utilities for Trident storage engine.
//!
//! This module provides:
//! - Unified magic number and version scheme
//! - CRC32 checksum computation and validation
//! - Version-aware format readers/writers
//! - Corruption detection and recovery helpers

pub mod binary;
pub mod codecs;

use crc32fast::Hasher as Crc32Hasher;
use std::io::{self, Read, Write};

/// Magic number identifying Trident index files: "TRID" (0x54524944)
pub const TRIDENT_MAGIC: u32 = 0x54524944;

/// Current binary format version
pub const FORMAT_VERSION: u16 = 1;

/// Index type identifiers for binary format headers
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum IndexType {
    Lsm = 1,
    Btree = 2,
    Adjacency = 3,
    Hnsw = 4,
}

impl IndexType {
    pub fn to_u32(self) -> u32 {
        self as u32
    }

    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(IndexType::Lsm),
            2 => Some(IndexType::Btree),
            3 => Some(IndexType::Adjacency),
            4 => Some(IndexType::Hnsw),
            _ => None,
        }
    }
}

/// Standard 32-byte binary format header
#[derive(Debug, Clone)]
pub struct FormatHeader {
    /// Magic: "TRID" (0x54524944)
    pub magic: u32,
    /// Version of format (currently 1)
    pub version: u16,
    /// Type of index (LSM, B-tree, Adjacency, HNSW)
    pub index_type: IndexType,
    /// CRC32 of all following bytes
    pub crc32: u32,
    /// Offset of variable-length metadata
    pub metadata_offset: u64,
    /// Unix timestamp in milliseconds when file was created
    pub timestamp: u64,
    /// Reserved for future use
    pub reserved: u16,
}

impl FormatHeader {
    /// Create a new header with current version and timestamp
    pub fn new(index_type: IndexType, metadata_offset: u64) -> Self {
        Self {
            magic: TRIDENT_MAGIC,
            version: FORMAT_VERSION,
            index_type,
            crc32: 0, // Will be set during write
            metadata_offset,
            timestamp: current_timestamp_millis(),
            reserved: 0,
        }
    }

    /// Serialize header to bytes (32 bytes)
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0..4].copy_from_slice(&self.magic.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.version.to_le_bytes());
        bytes[6..10].copy_from_slice(&self.index_type.to_u32().to_le_bytes());
        bytes[10..14].copy_from_slice(&self.crc32.to_le_bytes());
        bytes[14..22].copy_from_slice(&self.metadata_offset.to_le_bytes());
        bytes[22..30].copy_from_slice(&self.timestamp.to_le_bytes());
        bytes[30..32].copy_from_slice(&self.reserved.to_le_bytes());
        bytes
    }

    /// Deserialize header from bytes
    pub fn from_bytes(bytes: &[u8; 32]) -> io::Result<Self> {
        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if magic != TRIDENT_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid magic number: {:#x}", magic),
            ));
        }

        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unsupported format version: {}", version),
            ));
        }

        let index_type_u32 = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
        let index_type = IndexType::from_u32(index_type_u32).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid index type: {}", index_type_u32),
            )
        })?;

        let crc32 = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
        let metadata_offset = u64::from_le_bytes([
            bytes[14], bytes[15], bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21],
        ]);
        let timestamp = u64::from_le_bytes([
            bytes[22], bytes[23], bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29],
        ]);
        let reserved = u16::from_le_bytes([bytes[30], bytes[31]]);

        Ok(Self {
            magic,
            version,
            index_type,
            crc32,
            metadata_offset,
            timestamp,
            reserved,
        })
    }
}

/// CRC32-based corruption detection helper
pub struct ChecksumValidator {
    hasher: Crc32Hasher,
}

impl ChecksumValidator {
    /// Create new hasher
    pub fn new() -> Self {
        Self {
            hasher: Crc32Hasher::new(),
        }
    }

    /// Update hasher with data
    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    /// Finalize and return CRC32
    pub fn finalize(self) -> u32 {
        self.hasher.finalize()
    }
}

impl Default for ChecksumValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute CRC32 for arbitrary data
pub fn compute_crc32(data: &[u8]) -> u32 {
    let mut hasher = Crc32Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// Validate CRC32 of data
pub fn validate_crc32(data: &[u8], expected_crc: u32) -> bool {
    compute_crc32(data) == expected_crc
}

/// Get current timestamp in milliseconds
fn current_timestamp_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Binary format writer helper
pub struct BinaryWriter<W: Write> {
    writer: W,
    buffer: Vec<u8>,
}

impl<W: Write> BinaryWriter<W> {
    /// Create new binary writer
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            buffer: Vec::new(),
        }
    }

    /// Write a u32
    pub fn write_u32(&mut self, value: u32) -> io::Result<()> {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Write a u64
    pub fn write_u64(&mut self, value: u64) -> io::Result<()> {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Write a u16
    pub fn write_u16(&mut self, value: u16) -> io::Result<()> {
        self.buffer.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Write a u8
    pub fn write_u8(&mut self, value: u8) -> io::Result<()> {
        self.buffer.push(value);
        Ok(())
    }

    /// Write bytes
    pub fn write_bytes(&mut self, data: &[u8]) -> io::Result<()> {
        self.buffer.extend_from_slice(data);
        Ok(())
    }

    /// Flush buffered data to writer
    pub fn flush_buffer(&mut self) -> io::Result<()> {
        self.writer.write_all(&self.buffer)?;
        self.buffer.clear();
        Ok(())
    }

    /// Get current buffer size
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// Get buffered data reference
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// Take ownership of buffer
    pub fn take_buffer(self) -> Vec<u8> {
        self.buffer
    }
}

/// Binary format reader helper
pub struct BinaryReader<R: Read> {
    reader: R,
    buffer: Vec<u8>,
    position: usize,
}

impl<R: Read> BinaryReader<R> {
    /// Create new binary reader
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            position: 0,
        }
    }

    /// Read exactly n bytes into internal buffer
    pub fn read_exact(&mut self, n: usize) -> io::Result<()> {
        self.buffer.resize(n, 0);
        self.reader.read_exact(&mut self.buffer)?;
        self.position = 0;
        Ok(())
    }

    /// Read a u32
    pub fn read_u32(&mut self) -> io::Result<u32> {
        self.read_exact(4)?;
        Ok(u32::from_le_bytes([
            self.buffer[0],
            self.buffer[1],
            self.buffer[2],
            self.buffer[3],
        ]))
    }

    /// Read a u64
    pub fn read_u64(&mut self) -> io::Result<u64> {
        self.read_exact(8)?;
        Ok(u64::from_le_bytes([
            self.buffer[0],
            self.buffer[1],
            self.buffer[2],
            self.buffer[3],
            self.buffer[4],
            self.buffer[5],
            self.buffer[6],
            self.buffer[7],
        ]))
    }

    /// Read a u16
    pub fn read_u16(&mut self) -> io::Result<u16> {
        self.read_exact(2)?;
        Ok(u16::from_le_bytes([self.buffer[0], self.buffer[1]]))
    }

    /// Read a u8
    pub fn read_u8(&mut self) -> io::Result<u8> {
        self.read_exact(1)?;
        Ok(self.buffer[0])
    }

    /// Read exactly n bytes and return as Vec
    pub fn read_bytes(&mut self, n: usize) -> io::Result<Vec<u8>> {
        self.read_exact(n)?;
        Ok(self.buffer.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_format_header_roundtrip() {
        let header = FormatHeader::new(IndexType::Lsm, 256);
        let bytes = header.to_bytes();
        let decoded = FormatHeader::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.magic, TRIDENT_MAGIC);
        assert_eq!(decoded.version, FORMAT_VERSION);
        assert_eq!(decoded.index_type, IndexType::Lsm);
        assert_eq!(decoded.metadata_offset, 256);
    }

    #[test]
    fn test_crc32_validation() {
        let data = b"hello world";
        let crc = compute_crc32(data);
        assert!(validate_crc32(data, crc));
        assert!(!validate_crc32(data, crc + 1));
    }

    #[test]
    fn test_index_type_conversion() {
        for (value, expected) in [
            (1u32, IndexType::Lsm),
            (2, IndexType::Btree),
            (3, IndexType::Adjacency),
            (4, IndexType::Hnsw),
        ] {
            let idx_type = IndexType::from_u32(value).unwrap();
            assert_eq!(idx_type, expected);
            assert_eq!(idx_type.to_u32(), value);
        }

        assert!(IndexType::from_u32(99).is_none());
    }

    #[test]
    fn test_binary_writer() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = BinaryWriter::new(cursor);

        writer.write_u32(12345).unwrap();
        writer.write_u64(67890).unwrap();
        writer.write_u16(100).unwrap();
        writer.write_u8(42).unwrap();

        let buffer = writer.take_buffer();
        assert_eq!(buffer.len(), 4 + 8 + 2 + 1);
    }

    #[test]
    fn test_binary_reader() {
        let data = vec![
            123, 45, 0, 0, // u32 = 11643
            200, 10, 0, 0, 0, 0, 0, 0, // u64
            50, 0,  // u16 = 50
            99, // u8
        ];

        let cursor = Cursor::new(data);
        let mut reader = BinaryReader::new(cursor);

        let v1 = reader.read_u32().unwrap();
        assert_eq!(v1, 11643);

        let _v2 = reader.read_u64().unwrap();
        let v3 = reader.read_u16().unwrap();
        assert_eq!(v3, 50);

        let v4 = reader.read_u8().unwrap();
        assert_eq!(v4, 99);
    }
}
