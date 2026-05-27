#[allow(clippy::module_inception)]
pub mod errors;

pub use errors::PraxisError;
pub type Result<T> = std::result::Result<T, PraxisError>;
