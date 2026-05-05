pub const SEGMENT_MAGIC: u32 = 0x5453_4547;
pub const SEGMENT_VERSION: u32 = 2;
pub const SEGMENT_FOOTER_MAGIC: u32 = 0x5453_4654;
pub const SEGMENT_HEADER_LEN: usize = 17;
pub const SEGMENT_FOOTER_TRAILER_LEN: usize = 12;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockIndexEntry {
    pub first_key: Vec<u8>,
    pub last_key: Vec<u8>,
    pub offset: u64,
    pub len: u64,
}
