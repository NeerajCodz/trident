use crate::errors::{Result, TridentError};
use crate::replication::{LogPosition, ReplicationLog, ReplicationRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Raft node state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RaftState {
    Follower,
    Candidate,
    Leader,
}

/// Raft log entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RaftEntry {
    pub term: u64,
    pub index: u64,
    pub payload: Vec<u8>,
}

/// Raft vote request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VoteRequest {
    pub term: u64,
    pub candidate_id: String,
    pub last_log_index: u64,
    pub last_log_term: u64,
}

/// Raft vote response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VoteResponse {
    pub term: u64,
    pub vote_granted: bool,
}

/// Raft append entries request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppendEntriesRequest {
    pub term: u64,
    pub leader_id: String,
    pub prev_log_index: u64,
    pub prev_log_term: u64,
    pub entries: Vec<RaftEntry>,
    pub leader_commit: u64,
}

/// Raft append entries response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppendEntriesResponse {
    pub term: u64,
    pub success: bool,
    pub match_index: u64,
}

/// Raft consensus implementation.
///
/// This implements the core Raft algorithm:
/// - Leader election
/// - Log replication
/// - Safety guarantees
///
/// The implementation is designed to work with Trident's existing
/// FileReplicationLog for persistent storage.
pub struct RaftNode {
    /// This node's ID
    pub id: String,
    /// Current term
    pub current_term: u64,
    /// Who we voted for in current term
    pub voted_for: Option<String>,
    /// Current state (Follower, Candidate, Leader)
    pub state: RaftState,
    /// The log entries
    pub log: Vec<RaftEntry>,
    /// Index of highest log entry known to be committed
    pub commit_index: u64,
    /// Index of highest log entry applied to state machine
    pub last_applied: u64,
    /// For each peer, the next log entry to send
    pub next_index: BTreeMap<String, u64>,
    /// For each peer, the highest log entry known to be replicated
    pub match_index: BTreeMap<String, u64>,
    /// Known peers
    pub peers: Vec<String>,
    /// Election timeout ticks remaining
    pub election_timeout: u64,
    /// Heartbeat interval ticks
    pub heartbeat_interval: u64,
    /// Ticks since last heartbeat
    pub ticks_since_heartbeat: u64,
}

impl RaftNode {
    pub fn new(id: impl Into<String>, peers: Vec<String>) -> Self {
        Self {
            id: id.into(),
            current_term: 0,
            voted_for: None,
            state: RaftState::Follower,
            log: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            next_index: BTreeMap::new(),
            match_index: BTreeMap::new(),
            peers,
            election_timeout: 150,
            heartbeat_interval: 50,
            ticks_since_heartbeat: 0,
        }
    }

    /// Get the last log index.
    pub fn last_log_index(&self) -> u64 {
        self.log.last().map(|e| e.index).unwrap_or(0)
    }

    /// Get the last log term.
    pub fn last_log_term(&self) -> u64 {
        self.log.last().map(|e| e.term).unwrap_or(0)
    }

    /// Append a new entry to the log (leader only).
    pub fn append_entry(&mut self, payload: Vec<u8>) -> Result<RaftEntry> {
        if self.state != RaftState::Leader {
            return Err(TridentError::Replication("only leader can append entries".into()));
        }

        let entry = RaftEntry {
            term: self.current_term,
            index: self.last_log_index() + 1,
            payload,
        };
        self.log.push(entry.clone());
        Ok(entry)
    }

    /// Handle a vote request from a candidate.
    pub fn handle_vote_request(&mut self, request: VoteRequest) -> VoteResponse {
        // If term is newer, become follower
        if request.term > self.current_term {
            self.become_follower(request.term);
        }

        let mut vote_granted = false;

        if request.term >= self.current_term {
            let can_vote = self.voted_for.is_none()
                || self.voted_for.as_ref() == Some(&request.candidate_id);

            let log_ok = request.last_log_term > self.last_log_term()
                || (request.last_log_term == self.last_log_term()
                    && request.last_log_index >= self.last_log_index());

            if can_vote && log_ok {
                self.voted_for = Some(request.candidate_id);
                self.election_timeout = 150; // reset election timeout
                vote_granted = true;
            }
        }

        VoteResponse {
            term: self.current_term,
            vote_granted,
        }
    }

    /// Handle an append entries request from the leader.
    pub fn handle_append_entries(&mut self, request: AppendEntriesRequest) -> AppendEntriesResponse {
        // If term is newer, become follower
        if request.term > self.current_term {
            self.become_follower(request.term);
        }

        // Reject if term is older
        if request.term < self.current_term {
            return AppendEntriesResponse {
                term: self.current_term,
                success: false,
                match_index: 0,
            };
        }

        // Reset election timeout (we heard from leader)
        self.election_timeout = 150;
        self.ticks_since_heartbeat = 0;

        // Check log consistency
        if request.prev_log_index > 0 {
            if let Some(entry) = self.log.get((request.prev_log_index - 1) as usize) {
                if entry.term != request.prev_log_term {
                    // Delete conflicting entries
                    self.log.truncate((request.prev_log_index - 1) as usize);
                    return AppendEntriesResponse {
                        term: self.current_term,
                        success: false,
                        match_index: self.last_log_index(),
                    };
                }
            } else {
                return AppendEntriesResponse {
                    term: self.current_term,
                    success: false,
                    match_index: self.last_log_index(),
                };
            }
        }

        // Append new entries
        for entry in request.entries {
            if let Some(existing) = self.log.get((entry.index - 1) as usize) {
                if existing.term != entry.term {
                    self.log.truncate((entry.index - 1) as usize);
                    self.log.push(entry);
                }
            } else {
                self.log.push(entry);
            }
        }

        // Update commit index
        if request.leader_commit > self.commit_index {
            self.commit_index = request.leader_commit.min(self.last_log_index());
        }

        AppendEntriesResponse {
            term: self.current_term,
            success: true,
            match_index: self.last_log_index(),
        }
    }

    /// Start an election (candidate state).
    pub fn start_election(&mut self) {
        self.state = RaftState::Candidate;
        self.current_term += 1;
        self.voted_for = Some(self.id.clone());
        self.election_timeout = 150;
    }

    /// Become a leader (after winning election).
    pub fn become_leader(&mut self) {
        self.state = RaftState::Leader;
        let last_idx = self.last_log_index();
        for peer in &self.peers {
            self.next_index.insert(peer.clone(), last_idx + 1);
            self.match_index.insert(peer.clone(), 0);
        }
    }

    /// Become a follower.
    pub fn become_follower(&mut self, term: u64) {
        self.state = RaftState::Follower;
        self.current_term = term;
        self.voted_for = None;
    }

    /// Process a tick (called periodically).
    pub fn tick(&mut self) {
        self.ticks_since_heartbeat += 1;

        match self.state {
            RaftState::Follower | RaftState::Candidate => {
                if self.election_timeout > 0 {
                    self.election_timeout -= 1;
                }
            }
            RaftState::Leader => {
                if self.ticks_since_heartbeat >= self.heartbeat_interval {
                    self.ticks_since_heartbeat = 0;
                    // Would send heartbeats here
                }
            }
        }
    }

    /// Check if election timeout has expired.
    pub fn election_expired(&self) -> bool {
        self.election_timeout == 0
    }

    /// Get the majority count needed for quorum.
    pub fn quorum_size(&self) -> usize {
        (self.peers.len() + 1) / 2 + 1
    }

    /// Check if a log entry is committed (has been replicated to majority).
    pub fn is_committed(&self, index: u64) -> bool {
        index <= self.commit_index
    }

    /// Get entries that need to be applied to the state machine.
    pub fn entries_to_apply(&self) -> &[RaftEntry] {
        let start = self.last_applied as usize;
        let end = self.commit_index as usize;
        if start < end && end <= self.log.len() {
            &self.log[start..end]
        } else {
            &[]
        }
    }

    /// Mark entries as applied.
    pub fn applied_up_to(&mut self, index: u64) {
        self.last_applied = index;
    }

    /// Create an append entries request for a specific peer.
    pub fn create_append_request(&self, peer: &str) -> Option<AppendEntriesRequest> {
        let next = self.next_index.get(peer).copied().unwrap_or(1);
        let prev_index = next.saturating_sub(1);
        let prev_term = if prev_index > 0 {
            self.log.get((prev_index - 1) as usize).map(|e| e.term).unwrap_or(0)
        } else {
            0
        };

        let entries: Vec<RaftEntry> = self.log
            .iter()
            .skip((next - 1) as usize)
            .cloned()
            .collect();

        Some(AppendEntriesRequest {
            term: self.current_term,
            leader_id: self.id.clone(),
            prev_log_index: prev_index,
            prev_log_term: prev_term,
            entries,
            leader_commit: self.commit_index,
        })
    }

    /// Update match/next index for a peer after successful append.
    pub fn update_peer_state(&mut self, peer: &str, match_index: u64) {
        self.match_index.insert(peer.to_string(), match_index);
        self.next_index.insert(peer.to_string(), match_index + 1);

        // Try to advance commit index
        let mut sorted: Vec<u64> = self.match_index.values().copied().collect();
        sorted.push(self.last_log_index()); // include leader's own
        sorted.sort_unstable();

        let quorum_idx = sorted.len() - self.quorum_size();
        let new_commit = sorted[quorum_idx];

        if new_commit > self.commit_index {
            // Only commit if the entry is from current term
            if let Some(entry) = self.log.get((new_commit - 1) as usize) {
                if entry.term == self.current_term {
                    self.commit_index = new_commit;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raft_node_creation() {
        let node = RaftNode::new("node1", vec!["node2".to_string(), "node3".to_string()]);
        assert_eq!(node.state, RaftState::Follower);
        assert_eq!(node.current_term, 0);
        assert_eq!(node.quorum_size(), 2);
    }

    #[test]
    fn test_raft_election() {
        let mut node = RaftNode::new("node1", vec!["node2".to_string(), "node3".to_string()]);
        node.start_election();
        assert_eq!(node.state, RaftState::Candidate);
        assert_eq!(node.current_term, 1);
        assert_eq!(node.voted_for, Some("node1".to_string()));
    }

    #[test]
    fn test_raft_append_entry() {
        let mut node = RaftNode::new("node1", vec!["node2".to_string()]);
        node.become_leader();
        let entry = node.append_entry(b"test data".to_vec()).unwrap();
        assert_eq!(entry.index, 1);
        assert_eq!(entry.term, 0);
        assert_eq!(node.last_log_index(), 1);
    }

    #[test]
    fn test_raft_vote_request() {
        let mut node = RaftNode::new("node1", vec!["node2".to_string()]);

        let request = VoteRequest {
            term: 1,
            candidate_id: "node2".to_string(),
            last_log_index: 0,
            last_log_term: 0,
        };

        let response = node.handle_vote_request(request);
        assert!(response.vote_granted);
        assert_eq!(node.voted_for, Some("node2".to_string()));
    }

    #[test]
    fn test_raft_append_entries() {
        let mut node = RaftNode::new("node1", vec!["node2".to_string()]);

        let request = AppendEntriesRequest {
            term: 1,
            leader_id: "node2".to_string(),
            prev_log_index: 0,
            prev_log_term: 0,
            entries: vec![RaftEntry {
                term: 1,
                index: 1,
                payload: b"entry1".to_vec(),
            }],
            leader_commit: 0,
        };

        let response = node.handle_append_entries(request);
        assert!(response.success);
        assert_eq!(node.last_log_index(), 1);
    }
}
