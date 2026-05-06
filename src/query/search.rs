#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchQuery {
    pub terms: Vec<String>,
    pub fields: Vec<String>,
    pub limit: usize,
}
