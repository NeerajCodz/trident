pub mod context;
pub mod errors;
pub mod level;
pub mod logger;

pub use context::Context;
pub use errors::{Result, TridentError};
pub use level::Level;
pub use logger::Logger;

pub fn context() -> Context {
    Context::new()
}

pub fn info(event: &str, context: Context) {
    Logger::global().emit(Level::Info, event, context);
}

pub fn warn(event: &str, context: Context) {
    Logger::global().emit(Level::Warn, event, context);
}

pub fn warning(event: &str, context: Context) {
    warn(event, context);
}

pub fn error(event: &str, context: Context) {
    Logger::global().emit(Level::Error, event, context);
}

pub fn storage(event: &str, context: Context) {
    info(event, context.with_str("domain", "storage"));
}

pub fn index(event: &str, context: Context) {
    info(event, context.with_str("domain", "index"));
}

pub fn query(event: &str, context: Context) {
    info(event, context.with_str("domain", "query"));
}

pub fn accel(event: &str, context: Context) {
    info(event, context.with_str("domain", "accel"));
}

pub fn compaction(event: &str, context: Context) {
    info(event, context.with_str("domain", "compaction"));
}
