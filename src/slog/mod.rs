pub mod context;
pub mod errors;
pub mod level;
pub mod logger;
pub mod wide;

pub use context::Context;
pub use errors::{Result, TridentError};
pub use level::Level;
pub use logger::Logger;
pub use wide::WideEvent;

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

pub struct Info;

impl Info {
    pub fn emit(event: &str, context: Context) {
        info(event, context);
    }
}

pub struct Warning;

impl Warning {
    pub fn emit(event: &str, context: Context) {
        warn(event, context);
    }
}

pub struct Error;

impl Error {
    pub fn emit(event: &str, context: Context) {
        error(event, context);
    }
}

pub struct Storage;

impl Storage {
    pub fn emit(event: &str, context: Context) {
        storage(event, context);
    }
}

pub struct Index;

impl Index {
    pub fn emit(event: &str, context: Context) {
        index(event, context);
    }
}

pub struct Query;

impl Query {
    pub fn emit(event: &str, context: Context) {
        query(event, context);
    }
}

pub struct Accel;

impl Accel {
    pub fn emit(event: &str, context: Context) {
        accel(event, context);
    }
}

pub struct Compaction;

impl Compaction {
    pub fn emit(event: &str, context: Context) {
        compaction(event, context);
    }
}
