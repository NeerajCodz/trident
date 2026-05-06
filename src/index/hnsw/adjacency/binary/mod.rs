//! Adjacency list index binary format for graph edges
//!
//! Format:
//! - 32-byte FormatHeader
//! - AdjacencyMetadata (24 bytes): node_count, edge_count, generation, sequence
//! - Node entries: [node_id (8B) | edge_count (4B) | [neighbor_rids (8B each)] | [neighbor_nodes (8B each)]]

use crate::formats::{
    BinaryWriter, ChecksumValidator, FormatHeader, IndexType, TRIDENT_MAGIC,
};
use std::collections::BTreeMap;
use std::io::{self, Read, Write};

/// Adjacency metadata (24 bytes when serialized)
#[derive(Debug, Clone)]
pub struct AdjacencyBinaryMetadata {
    pub node_count: u32,    // Total nodes
    pub edge_count: u32,    // Total edges
    pub generation: u64,    // Compaction generation
    pub sequence: u64,      // WAL sequence number
}

impl AdjacencyBinaryMetadata {
    /// Serialize to 24 bytes
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut bytes = [0u8; 24];
        bytes[0..4].copy_from_slice(&self.node_count.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.edge_count.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.generation.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.sequence.to_le_bytes());
        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < 24 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Metadata too short",
            ));
        }

        Ok(Self {
            node_count: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            edge_count: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            generation: u64::from_le_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
            sequence: u64::from_le_bytes([
                bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22],
                bytes[23],
            ]),
        })
    }
}

/// Edge representation: (neighbor_node_id, neighbor_rid)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub neighbor_node: u64,
    pub neighbor_rid: u64,
}

/// Adjacency binary writer
pub struct AdjacencyBinaryWriter<W: Write> {
    writer: BinaryWriter<W>,
    metadata: AdjacencyBinaryMetadata,
    nodes: BTreeMap<u64, Vec<Edge>>, // node_id -> edges
}

impl<W: Write> AdjacencyBinaryWriter<W> {
    /// Create new adjacency binary writer
    pub fn new(writer: W, generation: u64, sequence: u64) -> Self {
        Self {
            writer: BinaryWriter::new(writer),
            metadata: AdjacencyBinaryMetadata {
                node_count: 0,
                edge_count: 0,
                generation,
                sequence,
            },
            nodes: BTreeMap::new(),
        }
    }

    /// Add edge from node_id to neighbor
    pub fn write_edge(&mut self, node_id: u64, neighbor_node: u64, neighbor_rid: u64) -> io::Result<()> {
        self.nodes
            .entry(node_id)
            .or_insert_with(Vec::new)
            .push(Edge {
                neighbor_node,
                neighbor_rid,
            });
        self.metadata.edge_count += 1;
        Ok(())
    }

    /// Finalize writer and compute checksum
    pub fn finish(mut self) -> io::Result<()> {
        // Count unique nodes
        self.metadata.node_count = self.nodes.len() as u32;

        // Write header (32 bytes)
        let metadata_offset = 32u64;
        let mut header = FormatHeader::new(IndexType::Adjacency, metadata_offset);

        // Prepare all data before computing checksum
        let mut content = Vec::new();

        // Write metadata (24 bytes)
        content.extend_from_slice(&self.metadata.to_bytes());

        // Write nodes with edges
        for (node_id, edges) in self.nodes.iter() {
            content.extend_from_slice(&node_id.to_le_bytes());
            content.extend_from_slice(&(edges.len() as u32).to_le_bytes());

            // Write edges
            for edge in edges {
                content.extend_from_slice(&edge.neighbor_node.to_le_bytes());
                content.extend_from_slice(&edge.neighbor_rid.to_le_bytes());
            }
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

/// Adjacency binary reader with corruption detection
#[derive(Debug)]
pub struct AdjacencyBinaryReader {
    _phantom: std::marker::PhantomData<()>,
    metadata: AdjacencyBinaryMetadata,
    nodes: BTreeMap<u64, Vec<Edge>>, // node_id -> edges
}

impl AdjacencyBinaryReader {
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
                "Invalid magic number for adjacency binary format",
            ));
        }
        if header.index_type != IndexType::Adjacency {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Index type mismatch (expected Adjacency)",
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
        if content.len() < 24 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Content too short for metadata",
            ));
        }

        let metadata = AdjacencyBinaryMetadata::from_bytes(&content[0..24])?;
        let mut nodes = BTreeMap::new();
        let mut pos = 24;

        // Parse node entries
        for _ in 0..metadata.node_count {
            if pos + 12 > content.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Truncated node entry",
                ));
            }

            let node_id = u64::from_le_bytes([
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

            let edge_count = u32::from_le_bytes([
                content[pos],
                content[pos + 1],
                content[pos + 2],
                content[pos + 3],
            ]) as usize;
            pos += 4;

            let mut edges = Vec::new();
            for _ in 0..edge_count {
                if pos + 16 > content.len() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Truncated edge data",
                    ));
                }

                let neighbor_node = u64::from_le_bytes([
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

                let neighbor_rid = u64::from_le_bytes([
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

                edges.push(Edge {
                    neighbor_node,
                    neighbor_rid,
                });
            }

            nodes.insert(node_id, edges);
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

    /// Get edge count
    pub fn edge_count(&self) -> u32 {
        self.metadata.edge_count
    }

    /// Get edges for node
    pub fn get_edges(&self, node_id: u64) -> Option<Vec<Edge>> {
        self.nodes.get(&node_id).cloned()
    }

    /// Get neighbors for node
    pub fn get_neighbors(&self, node_id: u64) -> Option<Vec<u64>> {
        self.nodes
            .get(&node_id)
            .map(|edges| edges.iter().map(|e| e.neighbor_node).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_adjacency_binary_roundtrip() {
        // Write
        let mut buffer = Vec::new();
        let mut writer = AdjacencyBinaryWriter::new(&mut buffer, 1, 100);

        writer.write_edge(1, 2, 200).unwrap();
        writer.write_edge(1, 3, 300).unwrap();
        writer.write_edge(2, 1, 100).unwrap();
        writer.write_edge(2, 4, 400).unwrap();
        writer.finish().unwrap();

        // Read
        let cursor = Cursor::new(buffer);
        let reader = AdjacencyBinaryReader::open(cursor).unwrap();

        assert_eq!(reader.node_count(), 2);
        assert_eq!(reader.edge_count(), 4);

        let edges_1 = reader.get_edges(1).unwrap();
        assert_eq!(edges_1.len(), 2);
        assert!(edges_1.iter().any(|e| e.neighbor_node == 2 && e.neighbor_rid == 200));
        assert!(edges_1.iter().any(|e| e.neighbor_node == 3 && e.neighbor_rid == 300));

        let neighbors_2 = reader.get_neighbors(2).unwrap();
        assert_eq!(neighbors_2, vec![1, 4]);
    }

    #[test]
    fn test_adjacency_binary_empty() {
        let mut buffer = Vec::new();
        let writer = AdjacencyBinaryWriter::new(&mut buffer, 5, 50);
        writer.finish().unwrap();

        let cursor = Cursor::new(buffer);
        let reader = AdjacencyBinaryReader::open(cursor).unwrap();
        assert_eq!(reader.node_count(), 0);
        assert_eq!(reader.edge_count(), 0);
    }

    #[test]
    fn test_adjacency_binary_corruption_detection() {
        // Write
        let mut buffer = Vec::new();
        let mut writer = AdjacencyBinaryWriter::new(&mut buffer, 2, 200);
        writer.write_edge(1, 2, 100).unwrap();
        writer.finish().unwrap();

        // Corrupt CRC
        if buffer.len() >= 14 {
            buffer[10] ^= 0xFF;
        }

        // Try to read
        let cursor = Cursor::new(buffer);
        let result = AdjacencyBinaryReader::open(cursor);
        assert!(result.is_err());
        let err_str = result.unwrap_err().to_string();
        assert!(err_str.contains("CRC32") || err_str.contains("mismatch"));
    }
}
