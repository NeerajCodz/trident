#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KvQuery {
    Get(Vec<u8>),
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete(Vec<u8>),
}
