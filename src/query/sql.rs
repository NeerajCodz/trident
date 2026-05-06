#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlQuery {
    pub statement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SqlIndexSpec {
    pub table: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub covering: Vec<String>,
}
