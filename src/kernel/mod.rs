use crate::errors::Result;
use crate::store::RecordId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionMode {
    Unified,
    Specialized,
    Hybrid,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KernelSnapshot {
    pub sequence: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KernelCompactionReport {
    pub records_retained: u64,
    pub records_dropped: u64,
    pub bytes_rewritten: u64,
}

pub trait StorageKernel {
    fn put_record(&mut self, bytes: &[u8]) -> Result<RecordId>;
    fn get_record(&self, rid: RecordId) -> Result<Vec<u8>>;
    fn delete_record(&mut self, rid: RecordId) -> Result<()>;
    fn snapshot(&self) -> KernelSnapshot;
    fn flush(&mut self) -> Result<()>;
    fn compact(&mut self) -> Result<KernelCompactionReport>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchResult {
    AllSucceeded,
    PartialFailure { succeeded: usize, failed: usize },
}

pub trait BatchKernel: StorageKernel {
    fn put_records_batch(&mut self, records: &[&[u8]]) -> Result<Vec<RecordId>> {
        records.iter().map(|r| self.put_record(r)).collect()
    }

    fn get_records_batch(&self, rids: &[RecordId]) -> Vec<Result<Vec<u8>>> {
        rids.iter().map(|rid| self.get_record(*rid)).collect()
    }

    fn delete_records_batch(&mut self, rids: &[RecordId]) -> Result<BatchResult> {
        let mut succeeded = 0usize;
        let mut failed = 0usize;
        for rid in rids {
            match self.delete_record(*rid) {
                Ok(()) => succeeded += 1,
                Err(_) => failed += 1,
            }
        }
        if failed == 0 {
            Ok(BatchResult::AllSucceeded)
        } else {
            Ok(BatchResult::PartialFailure { succeeded, failed })
        }
    }
}

pub struct ArenaBlock {
    data: Vec<u8>,
    offset: usize,
}

impl ArenaBlock {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: vec![0u8; capacity],
            offset: 0,
        }
    }

    pub fn alloc(&mut self, size: usize, align: usize) -> Option<&mut [u8]> {
        let aligned_offset = (self.offset + align - 1) & !(align - 1);
        if aligned_offset + size > self.data.len() {
            return None;
        }
        let start = aligned_offset;
        self.offset = aligned_offset + size;
        Some(&mut self.data[start..start + size])
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.offset)
    }

    pub fn used(&self) -> usize {
        self.offset
    }

    pub fn reset(&mut self) {
        self.offset = 0;
    }
}

pub struct KernelArena {
    blocks: Vec<ArenaBlock>,
    block_size: usize,
}

impl KernelArena {
    pub fn new(block_size: usize) -> Self {
        Self {
            blocks: vec![ArenaBlock::new(block_size)],
            block_size,
        }
    }

    pub fn alloc(&mut self, size: usize, align: usize) -> &mut [u8] {
        if let Some(slice) = self.blocks.last_mut().and_then(|b| b.alloc(size, align)) {
            return unsafe { &mut *(slice as *mut [u8]) };
        }
        let new_block_size = self.block_size.max(size + align);
        self.blocks.push(ArenaBlock::new(new_block_size));
        self.blocks
            .last_mut()
            .unwrap()
            .alloc(size, align)
            .expect("fresh arena block must fit requested allocation")
    }

    pub fn alloc_copy(&mut self, src: &[u8]) -> &mut [u8] {
        let dest = self.alloc(src.len(), 8);
        dest.copy_from_slice(src);
        dest
    }

    pub fn total_allocated(&self) -> usize {
        self.blocks.iter().map(|b| b.used()).sum()
    }

    pub fn total_capacity(&self) -> usize {
        self.blocks.iter().map(|b| b.data.len()).sum()
    }

    pub fn reset(&mut self) {
        for block in &mut self.blocks {
            block.reset();
        }
        if self.blocks.len() > 1 {
            self.blocks.truncate(1);
        }
    }
}

#[repr(C, align(64))]
pub struct CacheAlignedBuffer {
    data: Vec<u8>,
}

impl CacheAlignedBuffer {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0u8; size],
        }
    }

    pub fn from_slice(src: &[u8]) -> Self {
        Self {
            data: src.to_vec(),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[repr(C, align(64))]
pub struct CacheAlignedF32Buffer {
    data: Vec<f32>,
}

impl CacheAlignedF32Buffer {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0.0f32; size],
        }
    }

    pub fn from_slice(src: &[f32]) -> Self {
        Self {
            data: src.to_vec(),
        }
    }

    pub fn as_slice(&self) -> &[f32] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [f32] {
        &mut self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct KernelStats {
    pub total_puts: u64,
    pub total_gets: u64,
    pub total_deletes: u64,
    pub total_flushes: u64,
    pub total_compactions: u64,
    pub arena_bytes_allocated: u64,
    pub arena_bytes_capacity: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_alloc_basic() {
        let mut arena = KernelArena::new(1024);
        let buf = arena.alloc(64, 8);
        assert_eq!(buf.len(), 64);
    }

    #[test]
    fn arena_alloc_copy() {
        let mut arena = KernelArena::new(1024);
        let src = b"hello world";
        let buf = arena.alloc_copy(src);
        assert_eq!(buf, src);
    }

    #[test]
    fn arena_grows() {
        let mut arena = KernelArena::new(64);
        let _ = arena.alloc(32, 1);
        let _ = arena.alloc(32, 1);
        let _ = arena.alloc(32, 1);
        assert!(arena.blocks.len() >= 2);
    }

    #[test]
    fn arena_reset() {
        let mut arena = KernelArena::new(1024);
        let _ = arena.alloc(512, 1);
        assert_eq!(arena.total_allocated(), 512);
        arena.reset();
        assert_eq!(arena.total_allocated(), 0);
    }

    #[test]
    fn cache_aligned_buffer() {
        let buf = CacheAlignedBuffer::from_slice(b"test data");
        assert_eq!(buf.as_slice(), b"test data");
        assert_eq!(buf.len(), 9);
    }

    #[test]
    fn cache_aligned_f32_buffer() {
        let buf = CacheAlignedF32Buffer::from_slice(&[1.0, 2.0, 3.0]);
        assert_eq!(buf.as_slice(), &[1.0, 2.0, 3.0]);
        assert_eq!(buf.len(), 3);
    }

    #[test]
    fn arena_block_remaining() {
        let mut block = ArenaBlock::new(256);
        assert_eq!(block.remaining(), 256);
        block.alloc(100, 1).unwrap();
        assert_eq!(block.remaining(), 156);
    }

    #[test]
    fn batch_result_variants() {
        let result = BatchResult::AllSucceeded;
        assert_eq!(result, BatchResult::AllSucceeded);

        let partial = BatchResult::PartialFailure {
            succeeded: 5,
            failed: 2,
        };
        assert_eq!(
            partial,
            BatchResult::PartialFailure {
                succeeded: 5,
                failed: 2
            }
        );
    }
}
