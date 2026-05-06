//! HNSW (Hierarchical Navigable Small World) vector index binary format
//!
//! Format:
//! - 32-byte FormatHeader
//! - HnswMetadata (40 bytes): num_vectors, vector_dim, max_layer, entry_point, generation, sequence
//! - Vector metadata entries: [vector_id (8B) | rid (8B) | layer (4B)]

use crate::formats::{
    BinaryWriter, ChecksumValidator, FormatHeader, IndexType, TRIDENT_MAGIC,
};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

/// HNSW metadata (36 bytes when serialized)
#[derive(Debug, Clone)]
pub struct HnswBinaryMetadata {
    pub num_vectors: u32,
    pub vector_dim: u32,
    pub max_layer: u32,
    pub entry_point: u64,
    pub generation: u64,
    pub sequence: u64,
}

impl HnswBinaryMetadata {
    /// Serialize to 36 bytes
    pub fn to_bytes(&self) -> [u8; 36] {
        let mut bytes = [0u8; 36];
        bytes[0..4].copy_from_slice(&self.num_vectors.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.vector_dim.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.max_layer.to_le_bytes());
        bytes[12..20].copy_from_slice(&self.entry_point.to_le_bytes());
        bytes[20..28].copy_from_slice(&self.generation.to_le_bytes());
        bytes[28..36].copy_from_slice(&self.sequence.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 36 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Metadata too short",
            ));
        }

        Ok(Self {
            num_vectors: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            vector_dim: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            max_layer: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            entry_point: u64::from_le_bytes([
                bytes[12], bytes[13], bytes[14], bytes[15], bytes[16], bytes[17], bytes[18],
                bytes[19],
            ]),
            generation: u64::from_le_bytes([
                bytes[20], bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26],
                bytes[27],
            ]),
            sequence: u64::from_le_bytes([
                bytes[28], bytes[29], bytes[30], bytes[31], bytes[32], bytes[33], bytes[34],
                bytes[35],
            ]),
        })
    }
}

/// Vector node in HNSW graph
#[derive(Debug, Clone)]
pub struct VectorNode {
    pub vector_id: u64,
    pub rid: u64,
    pub layer: u32,
}

/// HNSW binary writer
pub struct HnswBinaryWriter<W: Write> {
    writer: BinaryWriter<W>,
    metadata: HnswBinaryMetadata,
    vectors: Vec<VectorNode>,
}

impl<W: Write> HnswBinaryWriter<W> {
    /// Create new HNSW binary writer
    pub fn new(
        writer: W,
        vector_dim: u32,
        entry_point: u64,
        generation: u64,
        sequence: u64,
    ) -> Self {
        Self {
            writer: BinaryWriter::new(writer),
            metadata: HnswBinaryMetadata {
                num_vectors: 0,
                vector_dim,
                max_layer: 0,
                entry_point,
                generation,
                sequence,
            },
            vectors: Vec::new(),
        }
    }

    /// Add vector to index
    pub fn add_vector(&mut self, vector_id: u64, rid: u64, layer: u32) -> io::Result<()> {
        self.vectors.push(VectorNode {
            vector_id,
            rid,
            layer,
        });
        self.metadata.num_vectors += 1;
        if layer > self.metadata.max_layer {
            self.metadata.max_layer = layer;
        }
        Ok(())
    }

    /// Finalize writer and compute checksum
    pub fn finish(mut self) -> io::Result<()> {
        // Write header (32 bytes)
        let metadata_offset = 32u64;
        let mut header = FormatHeader::new(IndexType::Hnsw, metadata_offset);

        // Prepare all data before computing checksum
        let mut content = Vec::new();

        // Write metadata (36 bytes)
        content.extend_from_slice(&self.metadata.to_bytes());

        // Write vectors
        for vector in self.vectors.iter() {
            content.extend_from_slice(&vector.vector_id.to_le_bytes());
            content.extend_from_slice(&vector.rid.to_le_bytes());
            content.extend_from_slice(&vector.layer.to_le_bytes());
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

/// HNSW binary reader with corruption detection
#[derive(Debug)]
pub struct HnswBinaryReader {
    _phantom: std::marker::PhantomData<()>,
    metadata: HnswBinaryMetadata,
    vectors: BTreeMap<u64, VectorNode>,
}

impl HnswBinaryReader {
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
                "Invalid magic number for HNSW binary format",
            ));
        }
        if header.index_type != IndexType::Hnsw {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Index type mismatch (expected HNSW)",
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
        if content.len() < 36 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Content too short for metadata",
            ));
        }

        let metadata = HnswBinaryMetadata::from_bytes(&content[0..36])?;
        let mut vectors = BTreeMap::new();
        let mut pos = 36;

        // Parse vector entries
        for _ in 0..metadata.num_vectors {
            if pos + 20 > content.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Truncated vector entry",
                ));
            }

            let vector_id = u64::from_le_bytes([
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

            let layer = u32::from_le_bytes([
                content[pos],
                content[pos + 1],
                content[pos + 2],
                content[pos + 3],
            ]);
            pos += 4;

            vectors.insert(
                vector_id,
                VectorNode {
                    vector_id,
                    rid,
                    layer,
                },
            );
        }

        Ok(Self {
            _phantom: std::marker::PhantomData,
            metadata,
            vectors,
        })
    }

    /// Get vector count
    pub fn vector_count(&self) -> u32 {
        self.metadata.num_vectors
    }

    /// Get vector dimension
    pub fn vector_dim(&self) -> u32 {
        self.metadata.vector_dim
    }

    /// Get vector metadata
    pub fn get_vector(&self, vector_id: u64) -> Option<&VectorNode> {
        self.vectors.get(&vector_id)
    }

    /// Get entry point RID
    pub fn entry_point_rid(&self) -> Option<u64> {
        self.vectors
            .get(&self.metadata.entry_point)
            .map(|v| v.rid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_hnsw_binary_roundtrip() {
        // Write
        let mut buffer = Vec::new();
        let mut writer = HnswBinaryWriter::new(&mut buffer, 128, 1, 1, 100);

        writer.add_vector(1, 1000, 2).unwrap();
        writer.add_vector(2, 2000, 2).unwrap();
        writer.add_vector(3, 3000, 1).unwrap();

        writer.finish().unwrap();

        // Read
        let cursor = Cursor::new(buffer);
        let reader = HnswBinaryReader::open(cursor).unwrap();

        assert_eq!(reader.vector_count(), 3);
        assert_eq!(reader.vector_dim(), 128);
        assert_eq!(reader.get_vector(1).unwrap().rid, 1000);
    }

    #[test]
    fn test_hnsw_binary_empty() {
        let mut buffer = Vec::new();
        let writer = HnswBinaryWriter::new(&mut buffer, 256, 0, 5, 50);
        writer.finish().unwrap();

        let cursor = Cursor::new(buffer);
        let reader = HnswBinaryReader::open(cursor).unwrap();
        assert_eq!(reader.vector_count(), 0);
    }

    #[test]
    fn test_hnsw_binary_corruption_detection() {
        // Write
        let mut buffer = Vec::new();
        let mut writer = HnswBinaryWriter::new(&mut buffer, 64, 1, 1, 1);
        writer.add_vector(1, 100, 0).unwrap();
        writer.finish().unwrap();

        // Corrupt CRC
        if buffer.len() >= 14 {
            buffer[10] ^= 0xFF;
        }

        // Try to read
        let cursor = Cursor::new(buffer);
        let result = HnswBinaryReader::open(cursor);
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("CRC32") || err_str.contains("mismatch"));
    }
}
