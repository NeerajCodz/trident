use crate::config::CompactionStrategy;
use crate::manifest::SegmentMetadata;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompactionPlan {
    pub source_segment_ids: Vec<u64>,
    pub target_level: u32,
}

pub fn pick_compaction_plan(
    strategy: CompactionStrategy,
    segments: &[SegmentMetadata],
) -> CompactionPlan {
    if segments.len() < 2 {
        return CompactionPlan::default();
    }
    match strategy {
        CompactionStrategy::Leveled => pick_leveled_plan(segments),
        CompactionStrategy::Tiered => pick_tiered_plan(segments),
        CompactionStrategy::Universal => pick_universal_plan(segments),
    }
}

fn pick_leveled_plan(segments: &[SegmentMetadata]) -> CompactionPlan {
    let mut by_level = BTreeMap::<u32, Vec<&SegmentMetadata>>::new();
    for segment in segments {
        by_level.entry(segment.level).or_default().push(segment);
    }
    let mut best: Option<(u64, u32, u64, Vec<u64>)> = None;
    for (level, level_segments) in &by_level {
        let Some(next_segments) = by_level.get(&level.saturating_add(1)) else {
            continue;
        };
        for segment in level_segments {
            let mut overlaps = next_segments
                .iter()
                .filter(|candidate| key_range_overlap(segment, candidate))
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>();
            if overlaps.is_empty() {
                continue;
            }
            overlaps.sort_unstable();
            let overlap_pressure = overlaps.len() as u64;
            let mut plan = vec![segment.id];
            plan.extend(overlaps);
            let candidate = (overlap_pressure, *level, segment.id, plan.clone());
            let is_better = best.as_ref().is_none_or(|current| {
                overlap_pressure > current.0
                    || (overlap_pressure == current.0
                        && (*level < current.1 || (*level == current.1 && segment.id < current.2)))
            });
            if is_better {
                best = Some(candidate);
            }
        }
    }
    if let Some((_, level, _, source_segment_ids)) = best {
        return CompactionPlan {
            source_segment_ids,
            target_level: level.saturating_add(1),
        };
    }
    let mut l0 = segments
        .iter()
        .filter(|segment| segment.level == 0)
        .collect::<Vec<_>>();
    if l0.len() > 1 {
        l0.sort_by_key(|segment| segment.id);
        return CompactionPlan {
            source_segment_ids: l0.into_iter().map(|segment| segment.id).collect(),
            target_level: 1,
        };
    }
    let mut fallback = segments.iter().collect::<Vec<_>>();
    fallback.sort_by_key(|segment| (segment.level, segment.id));
    CompactionPlan {
        source_segment_ids: fallback
            .into_iter()
            .take(2)
            .map(|segment| segment.id)
            .collect(),
        target_level: 1,
    }
}

fn pick_tiered_plan(segments: &[SegmentMetadata]) -> CompactionPlan {
    let mut by_level = BTreeMap::<u32, Vec<&SegmentMetadata>>::new();
    for segment in segments {
        by_level.entry(segment.level).or_default().push(segment);
    }
    let mut ranked = by_level
        .into_iter()
        .filter_map(|(level, mut run)| {
            if run.len() < 3 {
                return None;
            }
            run.sort_by_key(|segment| segment.id);
            let total_entries = run.iter().map(|segment| segment.entries).sum::<u64>();
            Some((run.len(), total_entries, level, run))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.cmp(&left.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    if let Some((_, _, level, run)) = ranked.into_iter().next() {
        return CompactionPlan {
            source_segment_ids: run.into_iter().take(4).map(|segment| segment.id).collect(),
            target_level: level.saturating_add(1),
        };
    }
    let mut fallback = segments.iter().collect::<Vec<_>>();
    fallback.sort_by_key(|segment| (segment.level, segment.id));
    let level = fallback.first().map(|segment| segment.level).unwrap_or(0);
    CompactionPlan {
        source_segment_ids: fallback
            .into_iter()
            .take(2)
            .map(|segment| segment.id)
            .collect(),
        target_level: level.saturating_add(1),
    }
}

fn pick_universal_plan(segments: &[SegmentMetadata]) -> CompactionPlan {
    let mut ordered = segments.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|segment| (segment.entries.max(1), segment.id));
    let smallest = ordered
        .first()
        .map(|segment| segment.entries.max(1))
        .unwrap_or(1);
    let largest = ordered
        .last()
        .map(|segment| segment.entries.max(1))
        .unwrap_or(1);
    let take = if largest >= smallest.saturating_mul(4) {
        3
    } else {
        2
    };
    let target_level = ordered
        .iter()
        .map(|segment| segment.level)
        .max()
        .unwrap_or(0)
        .max(1);
    CompactionPlan {
        source_segment_ids: ordered
            .into_iter()
            .take(take)
            .map(|segment| segment.id)
            .collect(),
        target_level,
    }
}

fn key_range_overlap(left: &SegmentMetadata, right: &SegmentMetadata) -> bool {
    !(left.max_key.as_slice() < right.min_key.as_slice()
        || right.max_key.as_slice() < left.min_key.as_slice())
}
