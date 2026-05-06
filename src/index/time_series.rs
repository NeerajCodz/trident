use crate::store::RecordId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeSeriesPoint {
    pub timestamp_ms: i64,
    pub rid: RecordId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimePartition {
    pub start_ms: i64,
    pub end_ms: i64,
    points: Vec<TimeSeriesPoint>,
}

impl TimePartition {
    pub fn new(start_ms: i64, end_ms: i64) -> Self {
        Self {
            start_ms,
            end_ms,
            points: Vec::new(),
        }
    }

    pub fn insert(&mut self, timestamp_ms: i64, rid: RecordId) -> bool {
        if timestamp_ms < self.start_ms || timestamp_ms > self.end_ms {
            return false;
        }
        self.points.push(TimeSeriesPoint { timestamp_ms, rid });
        true
    }

    pub fn range(&self, start_ms: i64, end_ms: i64) -> Vec<RecordId> {
        self.points
            .iter()
            .filter_map(|point| {
                (point.timestamp_ms >= start_ms && point.timestamp_ms <= end_ms)
                    .then_some(point.rid)
            })
            .collect()
    }
}
