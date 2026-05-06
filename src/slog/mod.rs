pub mod context;
pub mod level;
pub mod logger;
pub mod errors;

pub use context::Context;
pub use level::Level;
pub use logger::Logger;
pub use errors::{TridentError, Result};

pub fn context() -> Context {
    Context::new()
}

pub fn info(event: &str, context: Context) {
    Logger::global().emit(Level::Info, event, context);
}

pub fn warn(event: &str, context: Context) {
    Logger::global().emit(Level::Warn, event, context);
}

pub fn error(event: &str, context: Context) {
    Logger::global().emit(Level::Error, event, context);
}
