use crate::errors::{PraxisError, Result};
use crate::replication::ReplicationRecord;
use crate::replication::raft::{
    AppendEntriesRequest, AppendEntriesResponse, VoteRequest, VoteResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Transport message types for Raft communication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RaftMessage {
    VoteRequest(VoteRequest),
    VoteResponse(VoteResponse),
    AppendEntries(AppendEntriesRequest),
    AppendEntriesResponse(AppendEntriesResponse),
    /// Replication record for sync
    Replicate(ReplicationRecord),
    /// Acknowledge receipt of replication record
    ReplicateAck {
        sequence: u64,
    },
    /// Request snapshot
    SnapshotRequest {
        from_sequence: u64,
    },
    /// Snapshot chunk
    SnapshotChunk {
        snapshot_id: u64,
        chunk_id: u64,
        data: Vec<u8>,
        final_chunk: bool,
    },
}

/// Transport endpoint for a remote node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransportEndpoint {
    pub node_id: String,
    pub address: String,
    pub port: u16,
}

/// Trait for network transport implementations.
///
/// Implementors provide the actual network communication.
/// This allows plugging in different transport backends (TCP, gRPC, etc.)
pub trait ReplicationTransport: Send + Sync {
    /// Send a message to a specific node.
    fn send(&self, target: &str, message: RaftMessage) -> Result<()>;

    /// Receive a message (blocking).
    fn receive(&self) -> Result<(String, RaftMessage)>;

    /// Try to receive a message (non-blocking).
    fn try_receive(&self) -> Result<Option<(String, RaftMessage)>>;

    /// Get the list of connected peers.
    fn peers(&self) -> Vec<String>;

    /// Add a peer endpoint.
    fn add_peer(&self, endpoint: TransportEndpoint) -> Result<()>;

    /// Remove a peer.
    fn remove_peer(&self, node_id: &str) -> Result<()>;
}

/// Shared message bus for in-memory transports.
type MessageBus = Arc<Mutex<Vec<(String, RaftMessage)>>>;

/// In-memory transport for testing and single-node operation.
pub struct MemoryTransport {
    node_id: String,
    peers: Arc<Mutex<BTreeMap<String, TransportEndpoint>>>,
    bus: MessageBus,
}

impl MemoryTransport {
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            peers: Arc::new(Mutex::new(BTreeMap::new())),
            bus: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a network of connected memory transports sharing a message bus.
    pub fn create_network(node_ids: &[&str]) -> Vec<Self> {
        let bus: MessageBus = Arc::new(Mutex::new(Vec::new()));
        let transports: Vec<Self> = node_ids
            .iter()
            .map(|id| Self {
                node_id: id.to_string(),
                peers: Arc::new(Mutex::new(BTreeMap::new())),
                bus: bus.clone(),
            })
            .collect();

        // Connect all transports to each other
        for (i, transport) in transports.iter().enumerate() {
            for (j, other) in transports.iter().enumerate() {
                if i != j {
                    let endpoint = TransportEndpoint {
                        node_id: other.node_id.clone(),
                        address: format!("memory://{}", other.node_id),
                        port: 0,
                    };
                    transport.add_peer(endpoint).unwrap();
                }
            }
        }

        transports
    }
}

impl ReplicationTransport for MemoryTransport {
    fn send(&self, target: &str, message: RaftMessage) -> Result<()> {
        self.bus.lock().unwrap().push((target.to_string(), message));
        Ok(())
    }

    fn receive(&self) -> Result<(String, RaftMessage)> {
        let mut bus = self.bus.lock().unwrap();
        let pos = bus.iter().position(|(target, _)| target == &self.node_id);
        match pos {
            Some(idx) => Ok(bus.remove(idx)),
            None => Err(PraxisError::Replication("no messages available".into())),
        }
    }

    fn try_receive(&self) -> Result<Option<(String, RaftMessage)>> {
        let mut bus = self.bus.lock().unwrap();
        let pos = bus.iter().position(|(target, _)| target == &self.node_id);
        Ok(pos.map(|idx| bus.remove(idx)))
    }

    fn peers(&self) -> Vec<String> {
        self.peers.lock().unwrap().keys().cloned().collect()
    }

    fn add_peer(&self, endpoint: TransportEndpoint) -> Result<()> {
        self.peers
            .lock()
            .unwrap()
            .insert(endpoint.node_id.clone(), endpoint);
        Ok(())
    }

    fn remove_peer(&self, node_id: &str) -> Result<()> {
        self.peers.lock().unwrap().remove(node_id);
        Ok(())
    }
}

/// Replication log transport that uses the FileReplicationLog.
pub struct LogTransport {
    _log: Arc<Mutex<crate::replication::FileReplicationLog>>,
    _peers: BTreeMap<String, TransportEndpoint>,
}

impl LogTransport {
    pub fn new(log: crate::replication::FileReplicationLog) -> Self {
        Self {
            _log: Arc::new(Mutex::new(log)),
            _peers: BTreeMap::new(),
        }
    }
}

/// Replication manager that coordinates between Raft and the storage engine.
pub struct ReplicationManager {
    pub node_id: String,
    pub transport: Box<dyn ReplicationTransport>,
    pub mode: crate::replication::ReplicationMode,
}

impl ReplicationManager {
    pub fn new(
        node_id: impl Into<String>,
        transport: impl ReplicationTransport + 'static,
        mode: crate::replication::ReplicationMode,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            transport: Box::new(transport),
            mode,
        }
    }

    /// Replicate a record to peers based on the replication mode.
    pub fn replicate(&self, record: ReplicationRecord) -> Result<()> {
        let message = RaftMessage::Replicate(record);
        let peers = self.transport.peers();

        match self.mode {
            crate::replication::ReplicationMode::Async => {
                // Fire and forget
                for peer in &peers {
                    let _ = self.transport.send(peer, message.clone());
                }
                Ok(())
            }
            crate::replication::ReplicationMode::Sync => {
                // Wait for all peers
                for peer in &peers {
                    self.transport.send(peer, message.clone())?;
                }
                Ok(())
            }
            crate::replication::ReplicationMode::Quorum => {
                // Wait for majority
                let quorum = peers.len().div_ceil(2);
                let mut acks = 0;
                for peer in &peers {
                    if self.transport.send(peer, message.clone()).is_ok() {
                        acks += 1;
                        if acks >= quorum {
                            return Ok(());
                        }
                    }
                }
                if acks >= quorum {
                    Ok(())
                } else {
                    Err(PraxisError::Replication("quorum not reached".into()))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_transport_send_receive() {
        let transports = MemoryTransport::create_network(&["node1", "node2"]);
        let node1 = &transports[0];
        let node2 = &transports[1];

        // node1 sends to node2
        node1
            .send("node2", RaftMessage::ReplicateAck { sequence: 42 })
            .unwrap();

        // node2 receives
        let (from, msg) = node2.try_receive().unwrap().unwrap();
        assert_eq!(from, "node2");
        match msg {
            RaftMessage::ReplicateAck { sequence } => assert_eq!(sequence, 42),
            _ => panic!("unexpected message type"),
        }
    }

    #[test]
    fn test_memory_transport_network() {
        let transports = MemoryTransport::create_network(&["a", "b", "c"]);
        assert_eq!(transports[0].peers().len(), 2);
        assert_eq!(transports[1].peers().len(), 2);
        assert_eq!(transports[2].peers().len(), 2);
    }
}
