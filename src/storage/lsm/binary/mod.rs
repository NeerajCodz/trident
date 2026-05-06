//! LSM index binary format with CRC32 corruption detection
//!
//! Format:
//! - 32-byte FormatHeader (magic, version, crc32, etc.)
//! - LsmBinaryMetadata (29 bytes): bloom_bits, bloom_hashes, entry_count, generation, sequence
//! - Key-value entries: [key_len (4B) | key_bytes | rid (8B)]

use crate::formats::{
    BinaryWriter, ChecksumValidator, FormatHeader, IndexType, TRIDENT_MAGIC,
};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

/// LSM metadata (29 bytes when serialized)
#[derive(Debug, Clone)]
pub struct LsmBinaryMetadata {
    pub bloom_bits: u64,     // Bloom filter size in bits
    pub bloom_hashes: u8,    // Number of hash functions
    pub entry_count: u32,    // Number of entries
    pub generation: u64,     // Compaction generation
    pub sequence: u64,       // WAL sequence number
}

impl LsmBinaryMetadata {
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(29);
        bytes.extend_from_slice(&self.bloom_bits.to_le_bytes());
        bytes.push(self.bloom_hashes);
        bytes.extend_from_slice(&self.entry_count.to_le_bytes());
        bytes.extend_from_slice(&self.generation.to_le_bytes());
        bytes.extend_from_slice(&self.sequence.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 29 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Metadata too short",
            ));
        }

        Ok(Self {
            bloom_bits: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            bloom_hashes: bytes[8],
            entry_count: u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]),
            generation: u64::from_le_bytes([
                bytes[13], bytes[14], bytes[15], bytes[16], bytes[17], bytes[18], bytes[19],
                bytes[20],
            ]),
            sequence: u64::from_le_bytes([
                bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26], bytes[27],
                bytes[28],
            ]),
        })
    }
}

/// LSM binary writer
pub struct LsmBinaryWriter<W: Write> {
    writer: BinaryWriter<W>,
    metadata: LsmBinaryMetadata,
    entries: Vec<(Vec<u8>, u64)>,
}

impl<W: Write> LsmBinaryWriter<W> {
    /// Create new LSM binary writer
    pub fn new(writer: W, generation: u64, sequence: u64) -> Self {
        Self {
            writer: BinaryWriter::new(writer),
            metadata: LsmBinaryMetadata {
                bloom_bits: 8192,
                bloom_hashes: 2,
                entry_count: 0,
                generation,
                sequence,
            },
            entries: Vec::new(),
        }
    }

    /// Add key->RID entry
    pub fn write_entry(&mut self, key: Vec<u8>, rid: u64) -> io::Result<()> {
        self.entries.push((key, rid));
        self.metadata.entry_count += 1;
        Ok(())
    }

    /// Finalize writer and compute checksum
    pub fn finish(mut self) -> io::Result<()> {
        // Write header (32 bytes)
        let metadata_offset = 32u64;
        let mut header = FormatHeader::new(IndexType::Lsm, metadata_offset);

        // Prepare all data before computing checksum
        let mut content = Vec::new();

        // Write metadata (29 bytes)
        content.extend_from_slice(&self.metadata.to_bytes());

        // Write entries
        for (key, rid) in self.entries.iter() {
            // key_len (4 bytes) + key_bytes + rid (8 bytes)
            content.extend_from_slice(&(key.len() as u32).to_le_bytes());
            content.extend_from_slice(key);
            content.extend_from_slice(&rid.to_le_bytes());
        }

        // Compute CRC32 of all content
        let mut validator = ChecksumValidator::new();
        validator.update(&content);
        header.crc32 = validator.finalize();

        // Write header
        self.writer.write_bytes(&header.to_bytes())?;
        self.writer.flush_buffer()?;

        // Write content
        self.writer.write_bytes(&content)?;
        self.writer.flush_buffer()?;

        Ok(())
    }
}

/// LSM binary reader with corruption detection
#[derive(Debug)]
pub struct LsmBinaryReader {
    _phantom: std::marker::PhantomData<()>,
    metadata: LsmBinaryMetadata,
    entries: BTreeMap<Vec<u8>, u64>,
}

impl LsmBinaryReader {
    /// Open and validate binary format
    pub fn open<R: Read>(mut reader_inner: R) -> io::Result<Self> {
        // Read header (32 bytes)
        let mut header_bytes = [0u8; 32];
        reader_inner.read_exact(&mut header_bytes)?;
        let header = FormatHeader::from_bytes(&header_bytes)?;

        // Validate magic and type
        if header.magic != TRIDENT_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid magic number for LSM binary format",
            ));
        }
        if header.index_type != IndexType::Lsm {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Index type mismatch (expected LSM)",
            ));
        }

        // Read all remaining data
        let mut content = Vec::new();
        reader_inner.read_to_end(&mut content)?;

        // Validate CRC32
        let computed_crc = crate::formats::compute_crc32(&content);
        if computed_crc != header.crc32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "CRC32 mismatch: expected {}, got {}",
                    header.crc32, computed_crc
                ),
            ));
        }

        // Parse metadata
        if content.len() < 29 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Content too short for metadata",
            ));
        }

        let metadata = LsmBinaryMetadata::from_bytes(&content[0..29])?;
        let mut entries = BTreeMap::new();
        let mut pos = 29;

        // Parse entries
        for _ in 0..metadata.entry_count {
            if pos + 4 > content.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Truncated entry length",
                ));
            }

            let key_len = u32::from_le_bytes([
                content[pos],
                content[pos + 1],
                content[pos + 2],
                content[pos + 3],
            ]) as usize;
            pos += 4;

            if pos + key_len + 8 > content.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Truncated entry data",
                ));
            }

            let key = content[pos..pos + key_len].to_vec();
            pos += key_len;

            let rid = u64::from_le_bytes([
                content[pos],
                content[pos + 1],
                content[pos + 2],
                content[pos + 3],
                content[pos + 4],
                content[pos + 5],
                content[pos + 6],
                content[pos + 7],
            ]);
            pos += 8;

            entries.insert(key, rid);
        }

        // We don't need to keep the reader alive since all data is parsed into entries.
        Ok(Self {
            _phantom: std::marker::PhantomData,
            metadata,
            entries,
        })
    }

    /// Get entry count
    pub fn entry_count(&self) -> u32 {
        self.metadata.entry_count
    }

    /// Get RID for key
    pub fn get(&self, key: &[u8]) -> Option<u64> {
        self.entries.get(key).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_lsm_binary_roundtrip() {
        // Write to vec instead of cursor
        let mut buffer = Vec::new();
        let mut writer = LsmBinaryWriter::new(&mut buffer, 1, 100);

        writer.write_entry(b"key1".to_vec(), 1000).unwrap();
        writer.write_entry(b"key2".to_vec(), 2000).unwrap();
        writer.write_entry(b"key3".to_vec(), 3000).unwrap();

        // Finish writing - this consumes writer and returns result
        writer.finish().unwrap();

        // Read
        let cursor = Cursor::new(buffer);
        let reader = LsmBinaryReader::open(cursor).unwrap();

        assert_eq!(reader.entry_count(), 3);
        assert_eq!(reader.get(b"key1"), Some(1000));
        assert_eq!(reader.get(b"key2"), Some(2000));
        assert_eq!(reader.get(b"key3"), Some(3000));
        assert_eq!(reader.get(b"key4"), None);
    }

    #[test]
    fn test_lsm_binary_metadata() {
        let metadata = LsmBinaryMetadata {
            bloom_bits: 16384,
            bloom_hashes: 2,
            entry_count: 1000,
            generation: 42,
            sequence: 12345,
        };

        let bytes = metadata.to_bytes();
        let decoded = LsmBinaryMetadata::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.bloom_bits, 16384);
        assert_eq!(decoded.bloom_hashes, 2);
        assert_eq!(decoded.entry_count, 1000);
        assert_eq!(decoded.generation, 42);
        assert_eq!(decoded.sequence, 12345);
    }

    #[test]
    fn test_lsm_binary_corruption_detection() {
        // Write
        let mut buffer = Vec::new();
        let mut writer = LsmBinaryWriter::new(&mut buffer, 1, 100);
        writer.write_entry(b"test".to_vec(), 999).unwrap();
        writer.finish().unwrap();

        // Corrupt the CRC (at bytes 10-13 in header)
        if buffer.len() >= 14 {
            buffer[10] ^= 0xFF; // Flip bits in CRC
        }

        // Try to read - should fail
        let cursor = Cursor::new(buffer);
        let result = LsmBinaryReader::open(cursor);
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("CRC32") || err_str.contains("mismatch"));
    }

    #[test]
    fn test_lsm_binary_empty() {
        let mut buffer = Vec::new();
        let writer = LsmBinaryWriter::new(&mut buffer, 1, 100);
        writer.finish().unwrap();

        let cursor = Cursor::new(buffer);
        let reader = LsmBinaryReader::open(cursor).unwrap();
        assert_eq!(reader.entry_count(), 0);
    }
}
