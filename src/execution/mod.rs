//! Query execution engine: runs physical plans against a storage engine.

pub mod executor;

pub use executor::{Executor, RecordProvider, RecordStore, matches_boolean_expression, matches_predicate};
