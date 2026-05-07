use tempfile::tempdir;
use trident::replication::{
    FileReplicationLog, LogPosition, ReplicationLog, ReplicationRecord, ReplicationRecordKind,
};
use trident::store::RecordId;

#[test]
fn file_replication_log_replays_from_position() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("replication.trlog");
    let mut log = FileReplicationLog::open(&path).unwrap();

    log.append(&ReplicationRecord::put(1, RecordId(1), b"alpha"))
        .unwrap();
    log.append(&ReplicationRecord::put(2, RecordId(2), b"beta"))
        .unwrap();
    log.append(&ReplicationRecord::delete(3, RecordId(1)))
        .unwrap();

    let replay = log.replay_from(LogPosition { sequence: 2 }).unwrap();
    assert_eq!(replay.len(), 2);
    assert_eq!(replay[0].rid, Some(RecordId(2)));
    assert_eq!(replay[0].payload, b"beta");
    assert_eq!(replay[1].kind, ReplicationRecordKind::Delete);
}

#[test]
fn file_replication_log_stops_at_torn_suffix() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("replication.trlog");
    let mut log = FileReplicationLog::open(&path).unwrap();
    log.append(&ReplicationRecord::put(1, RecordId(1), b"alpha"))
        .unwrap();

    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(&[9, 9, 9]);
    std::fs::write(&path, bytes).unwrap();

    let replay = log.replay_from(LogPosition { sequence: 1 }).unwrap();
    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].payload, b"alpha");
}

#[test]
fn file_replication_log_rejects_corrupted_committed_record() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("replication.trlog");
    let mut log = FileReplicationLog::open(&path).unwrap();
    log.append(&ReplicationRecord::put(1, RecordId(1), b"alpha"))
        .unwrap();

    let mut bytes = std::fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&path, bytes).unwrap();

    assert!(log.replay_from(LogPosition { sequence: 1 }).is_err());
}
