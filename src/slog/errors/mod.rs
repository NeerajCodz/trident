pub mod errors;

pub use errors::TridentError;
pub type Result<T> = std::result::Result<T, TridentError>;
