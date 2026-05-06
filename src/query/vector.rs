#[derive(Clone, Debug, PartialEq)]
pub struct VectorQuery {
    pub vector: Vec<f32>,
    pub limit: usize,
    pub filter: Option<Vec<u8>>,
}
