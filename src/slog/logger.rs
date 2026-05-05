use crate::config::LoggingOptions;
use crate::slog::{Context, Level};
use serde_json::{Map, Value};
use std::io::Write;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static LOGGER: OnceLock<Logger> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct Logger {
    options: LoggingOptions,
}

impl Logger {
    pub fn init(options: LoggingOptions) {
        let _ = LOGGER.set(Logger { options });
    }

    pub fn global() -> &'static Logger {
        LOGGER.get_or_init(|| Logger {
            options: LoggingOptions::default(),
        })
    }

    pub fn emit(&self, level: Level, event: &str, context: Context) {
        let mut root = Map::new();
        root.insert("ts_unix_ms".to_string(), Value::from(now_unix_ms()));
        root.insert(
            "level".to_string(),
            Value::String(level.as_str().to_string()),
        );
        root.insert("event".to_string(), Value::String(event.to_string()));
        root.insert(
            "schema_version".to_string(),
            Value::String(self.options.schema_version.clone()),
        );
        root.insert(
            "service".to_string(),
            Value::String(self.options.service.clone()),
        );
        root.insert(
            "version".to_string(),
            Value::String(env!("CARGO_PKG_VERSION").to_string()),
        );
        root.insert(
            "commit".to_string(),
            Value::String(self.options.commit.clone()),
        );
        root.insert(
            "region".to_string(),
            Value::String(self.options.region.clone()),
        );
        root.insert("node".to_string(), Value::String(self.options.node.clone()));
        root.insert(
            "thread".to_string(),
            Value::String(format!("{:?}", std::thread::current().id())),
        );
        context.merge_into(&mut root);
        let line = serde_json::to_vec(&Value::Object(root)).unwrap_or_else(|_| b"{}".to_vec());
        match level {
            Level::Error => {
                let mut stderr = std::io::stderr().lock();
                let _ = stderr.write_all(&line);
                let _ = stderr.write_all(b"\n");
            }
            _ => {
                let mut stdout = std::io::stdout().lock();
                let _ = stdout.write_all(&line);
                let _ = stdout.write_all(b"\n");
            }
        }
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}
