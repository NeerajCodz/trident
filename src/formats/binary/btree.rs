//! B-tree index binary format with CRC32 corruption detection
//!
//! Format:
//! - 32-byte FormatHeader (magic, version, crc32, etc.)
//! - BtreeMetadata (32 bytes): order, height, node_count, root_rid, generation, sequence
//! - Node index entries: [page_id (8B) | rid (8B)] sorted by page_id

use crate::formats::{
    BinaryWriter, ChecksumValidator, FormatHeader, IndexType, TRIDENT_MAGIC,
};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

/// B-tree metadata (32 bytes when serialized)
#[derive(Debug, Clone)]
pub struct BtreeBinaryMetadata {
    pub order: u32,              // B-tree order (max keys per node)
    pub height: u32,             // Tree height
    pub node_count: u32,         // Total internal nodes
    pub root_rid: u64,           // RID of root node
    pub generation: u64,         // Compaction generation
    pub sequence: u64,           // WAL sequence number
}

impl BtreeBinaryMetadata {
    /// Serialize to 32 bytes
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0..4].copy_from_slice(&self.order.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.height.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.node_count.to_le_bytes());
        bytes[12..20].copy_from_slice(&self.root_rid.to_le_bytes());
        bytes[20..28].copy_from_slice(&self.generation.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.sequence.to_le_bytes()[..4]); // First 4 bytes of sequence
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Metadata too short",
            ));
        }

        Ok(Self {
            order: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            height: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            node_count: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            root_rid: u64::from_le_bytes([
                bytes[12], bytes[13], bytes[14], bytes[15], bytes[16], bytes[17], bytes[18],
                bytes[19],
            ]),
            generation: u64::from_le_bytes([
                bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26],
                bytes[27],
            ]),
            sequence: u64::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31], 0, 0, 0, 0]),
        })
    }
}

/// B-tree binary writer
pub struct BtreeBinaryWriter<W: Write> {
    writer: BinaryWriter<W>,
    metadata: BtreeBinaryMetadata,
    nodes: Vec<(u64, u64)>, // (page_id, rid) pairs
}

impl<W: Write> BtreeBinaryWriter<W> {
    /// Create new B-tree binary writer
    pub fn new(
        writer: W,
        order: u32,
        height: u32,
        root_rid: u64,
        generation: u64,
        sequence: u64,
    ) -> Self {
        Self {
            writer: BinaryWriter::new(writer),
            metadata: BtreeBinaryMetadata {
                order,
                height,
                node_count: 0,
                root_rid,
                generation,
                sequence,
            },
            nodes: Vec::new(),
        }
    }

    /// Add node reference (page_id -> RID)
    pub fn write_node(&mut self, page_id: u64, rid: u64) -> io::Result<()> {
        self.nodes.push((page_id, rid));
        self.metadata.node_count += 1;
        Ok(())
    }

    /// Finalize writer and compute checksum
    pub fn finish(mut self) -> io::Result<()> {
        // Write header (32 bytes)
        let metadata_offset = 32u64;
        let mut header = FormatHeader::new(IndexType::Btree, metadata_offset);

        // Prepare all data before computing checksum
        let mut content = Vec::new();

        // Write metadata (32 bytes)
        content.extend_from_slice(&self.metadata.to_bytes());

        // Write nodes sorted by page_id
        let mut sorted_nodes = self.nodes;
        sorted_nodes.sort_by_key(|n| n.0);

        for (page_id, rid) in sorted_nodes.iter() {
            content.extend_from_slice(&page_id.to_le_bytes());
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

/// B-tree binary reader with corruption detection
#[derive(Debug)]
pub struct BtreeBinaryReader {
    _phantom: std::marker::PhantomData<()>,
    metadata: BtreeBinaryMetadata,
    nodes: BTreeMap<u64, u64>, // page_id -> rid
}

impl BtreeBinaryReader {
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
                "Invalid magic number for B-tree binary format",
            ));
        }
        if header.index_type != IndexType::Btree {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Index type mismatch (expected B-tree)",
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
        if content.len() < 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Content too short for metadata",
            ));
        }

        let metadata = BtreeBinaryMetadata::from_bytes(&content[0..32])?;
        let mut nodes = BTreeMap::new();
        let mut pos = 32;

        // Parse node entries
        for _ in 0..metadata.node_count {
            if pos + 16 > content.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Truncated node entry",
                ));
            }

            let page_id = u64::from_le_bytes([
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

            nodes.insert(page_id, rid);
        }

        Ok(Self {
            _phantom: std::marker::PhantomData,
            metadata,
            nodes,
        })
    }

    /// Get node count
    pub fn node_count(&self) -> u32 {
        self.metadata.node_count
    }

    /// Get RID for page
    pub fn get_rid(&self, page_id: u64) -> Option<u64> {
        self.nodes.get(&page_id).copied()
    }

    /// Get root RID
    pub fn root_rid(&self) -> u64 {
        self.metadata.root_rid
    }

    /// Get tree height
    pub fn height(&self) -> u32 {
        self.metadata.height
    }

    /// Get tree order
    pub fn order(&self) -> u32 {
        self.metadata.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_btree_binary_roundtrip() {
        // Write
        let mut buffer = Vec::new();
        let mut writer = BtreeBinaryWriter::new(&mut buffer, 32, 3, 1000, 5, 12345);

        writer.write_node(0, 1000).unwrap();
        writer.write_node(1, 2000).unwrap();
        writer.write_node(2, 3000).unwrap();
        writer.finish().unwrap();

        // Read
        let cursor = Cursor::new(buffer);
        let reader = BtreeBinaryReader::open(cursor).unwrap();

        assert_eq!(reader.node_count(), 3);
        assert_eq!(reader.get_rid(0), Some(1000));
        assert_eq!(reader.get_rid(1), Some(2000));
        assert_eq!(reader.get_rid(2), Some(3000));
        assert_eq!(reader.root_rid(), 1000);
        assert_eq!(reader.height(), 3);
        assert_eq!(reader.order(), 32);
    }

    #[test]
    fn test_btree_binary_metadata() {
        let metadata = BtreeBinaryMetadata {
            order: 64,
            height: 5,
            node_count: 1000,
            root_rid: 5000,
            generation: 42,
            sequence: 99999,
        };

        let bytes = metadata.to_bytes();
        let decoded = BtreeBinaryMetadata::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.order, 64);
        assert_eq!(decoded.height, 5);
        assert_eq!(decoded.node_count, 1000);
        assert_eq!(decoded.root_rid, 5000);
        assert_eq!(decoded.generation, 42);
    }

    #[test]
    fn test_btree_binary_corruption_detection() {
        // Write
        let mut buffer = Vec::new();
        let mut writer = BtreeBinaryWriter::new(&mut buffer, 32, 2, 500, 1, 111);
        writer.write_node(0, 100).unwrap();
        writer.finish().unwrap();

        // Corrupt CRC
        if buffer.len() >= 14 {
            buffer[10] ^= 0xFF;
        }

        // Try to read
        let cursor = Cursor::new(buffer);
        let result = BtreeBinaryReader::open(cursor);
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("CRC32") || err_str.contains("mismatch"));
    }

    #[test]
    fn test_btree_binary_empty() {
        let mut buffer = Vec::new();
        let writer = BtreeBinaryWriter::new(&mut buffer, 32, 1, 0, 0, 0);
        writer.finish().unwrap();

        let cursor = Cursor::new(buffer);
        let reader = BtreeBinaryReader::open(cursor).unwrap();
        assert_eq!(reader.node_count(), 0);
        assert_eq!(reader.root_rid(), 0);
    }

    #[test]
    fn test_btree_binary_large_tree() {
        // Write large tree
        let mut buffer = Vec::new();
        let mut writer = BtreeBinaryWriter::new(&mut buffer, 128, 10, 99999, 123, 456789);

        for i in 0..1000 {
            writer.write_node(i, i * 100).unwrap();
        }
        writer.finish().unwrap();

        // Read and verify
        let cursor = Cursor::new(buffer);
        let reader = BtreeBinaryReader::open(cursor).unwrap();

        assert_eq!(reader.node_count(), 1000);
        assert_eq!(reader.order(), 128);
        assert_eq!(reader.height(), 10);

        for i in 0..1000 {
            assert_eq!(reader.get_rid(i), Some(i * 100));
        }
    }
}
