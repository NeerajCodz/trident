//! Structured logging for Trident storage engine.
//!
//! Provides environment-configurable logging with support for JSON output,
//! different log levels, and contextual metadata for debugging and production monitoring.

use std::io::Write;
use std::sync::Mutex;

/// Log level enumeration
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl LogLevel {
    pub fn from_env() -> Self {
        match std::env::var("TRIDENT_LOG_LEVEL")
            .as_deref()
            .unwrap_or("info")
        {
            "debug" => LogLevel::Debug,
            "warn" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }
}

/// Structured logging context for a single operation
#[derive(Clone, Debug)]
pub struct LogContext {
    pub level: LogLevel,
    pub module: String,
    pub message: String,
    pub fields: Vec<(String, String)>,
}

impl LogContext {
    pub fn new(level: LogLevel, module: &str, message: &str) -> Self {
        Self {
            level,
            module: module.to_string(),
            message: message.to_string(),
            fields: Vec::new(),
        }
    }

    pub fn with_field(mut self, key: &str, value: impl std::fmt::Display) -> Self {
        self.fields.push((key.to_string(), value.to_string()));
        self
    }

    pub fn with_fields(mut self, fields: Vec<(&str, String)>) -> Self {
        for (k, v) in fields {
            self.fields.push((k.to_string(), v));
        }
        self
    }

    /// Format as JSON Line (newline-delimited JSON)
    pub fn to_json_line(&self) -> String {
        let mut json = format!(
            r#"{{"level":"{}","module":"{}","message":"{}","timestamp":"{}""#,
            match self.level {
                LogLevel::Debug => "DEBUG",
                LogLevel::Info => "INFO",
                LogLevel::Warn => "WARN",
                LogLevel::Error => "ERROR",
            },
            self.module,
            self.message.replace("\"", "\\\""),
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        );

        for (k, v) in &self.fields {
            json.push_str(&format!(",\"{}\":\"{}\"", k, v.replace("\"", "\\\"")));
        }

        json.push('}');
        json
    }

    /// Format as plain text
    pub fn to_plain(&self) -> String {
        let mut s = format!(
            "[{}] {}: {}",
            match self.level {
                LogLevel::Debug => "DEBUG",
                LogLevel::Info => "INFO ",
                LogLevel::Warn => "WARN ",
                LogLevel::Error => "ERROR",
            },
            self.module,
            self.message
        );

        if !self.fields.is_empty() {
            s.push_str(" {");
            for (k, v) in &self.fields {
                s.push_str(&format!(" {}={}", k, v));
            }
            s.push('}');
        }

        s
    }
}

/// Global logger for the engine
pub struct EngineLogger {
    level: LogLevel,
    json_output: bool,
    writer: Mutex<Box<dyn Write + Send>>,
}

impl EngineLogger {
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        let level = LogLevel::from_env();
        let json_output = std::env::var("TRIDENT_LOG_FORMAT").as_deref() == Ok("json");

        Self {
            level,
            json_output,
            writer: Mutex::new(writer),
        }
    }

    pub fn log(&self, ctx: LogContext) {
        if ctx.level < self.level {
            return; // Skip logs below current level
        }

        let output = if self.json_output {
            ctx.to_json_line()
        } else {
            ctx.to_plain()
        };

        if let Ok(mut writer) = self.writer.lock() {
            let _ = writeln!(writer, "{}", output);
        }
    }

    pub fn debug(&self, module: &str, msg: &str) {
        self.log(LogContext::new(LogLevel::Debug, module, msg));
    }

    pub fn info(&self, module: &str, msg: &str) {
        self.log(LogContext::new(LogLevel::Info, module, msg));
    }

    pub fn warn(&self, module: &str, msg: &str) {
        self.log(LogContext::new(LogLevel::Warn, module, msg));
    }

    pub fn error(&self, module: &str, msg: &str) {
        self.log(LogContext::new(LogLevel::Error, module, msg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    #[test]
    fn log_context_json_format() {
        let ctx = LogContext::new(LogLevel::Info, "storage", "write completed")
            .with_field("rid", "12345")
            .with_field("bytes", "1024");

        let json = ctx.to_json_line();
        assert!(json.contains("\"level\":\"INFO\""));
        assert!(json.contains("\"module\":\"storage\""));
        assert!(json.contains("\"rid\":\"12345\""));
        assert!(json.contains("\"bytes\":\"1024\""));
    }

    #[test]
    fn log_context_plain_format() {
        let ctx =
            LogContext::new(LogLevel::Warn, "cache", "cache full").with_field("size_mb", "512");

        let plain = ctx.to_plain();
        assert!(plain.contains("[WARN ]"));
        assert!(plain.contains("cache"));
        assert!(plain.contains("cache full"));
        assert!(plain.contains("size_mb=512"));
    }

    #[test]
    fn engine_logger_respects_level() {
        let buf = Arc::new(StdMutex::new(Vec::new()));
        let buf_clone = Arc::clone(&buf);

        struct TestWriter(Arc<StdMutex<Vec<u8>>>);
        impl Write for TestWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let logger = EngineLogger {
            level: LogLevel::Warn,
            json_output: false,
            writer: Mutex::new(Box::new(TestWriter(buf_clone))),
        };

        logger.debug("test", "should be filtered");
        logger.info("test", "should be filtered");
        logger.warn("test", "should appear");

        let output = buf.lock().unwrap();
        let output_str = String::from_utf8_lossy(&output);
        assert!(!output_str.contains("should be filtered"));
        assert!(output_str.contains("should appear"));
    }
}
