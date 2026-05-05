use crate::config::CompactionStrategy;
use crate::manifest::{CompactionJobState, CompactionJobStatus, Manifest};

pub fn reserve_compaction_job(
    manifest: &mut Manifest,
    strategy: CompactionStrategy,
    source_segment_ids: Vec<u64>,
    output_segment_id: u64,
) -> u64 {
    let now = now_millis();
    let id = now.saturating_mul(1000).saturating_add(output_segment_id);
    manifest.compaction_jobs.push(CompactionJobState {
        id,
        strategy,
        status: CompactionJobStatus::Planned,
        source_segment_ids,
        output_segment_id: Some(output_segment_id),
        created_at_ms: now,
        updated_at_ms: now,
    });
    id
}

pub fn mark_running(manifest: &mut Manifest, job_id: u64) {
    if let Some(job) = manifest
        .compaction_jobs
        .iter_mut()
        .find(|existing| existing.id == job_id)
    {
        job.status = CompactionJobStatus::Running;
        job.updated_at_ms = now_millis();
    }
}

pub fn finish_job(
    manifest: &mut Manifest,
    job_id: u64,
    output_segment_id: Option<u64>,
    status: CompactionJobStatus,
) {
    if let Some(job) = manifest
        .compaction_jobs
        .iter_mut()
        .find(|existing| existing.id == job_id)
    {
        job.status = status;
        job.output_segment_id = output_segment_id;
        job.updated_at_ms = now_millis();
    }
}

pub fn reconcile_unfinished_jobs(manifest: &mut Manifest) -> bool {
    let mut changed = false;
    let now = now_millis();
    for job in &mut manifest.compaction_jobs {
        if matches!(
            job.status,
            CompactionJobStatus::Planned | CompactionJobStatus::Running
        ) {
            job.status = CompactionJobStatus::Aborted;
            job.updated_at_ms = now;
            changed = true;
        }
    }
    changed
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
