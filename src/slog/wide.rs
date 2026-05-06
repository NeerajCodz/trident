use crate::slog::Context;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct WideEvent {
    event: String,
    started_at: Instant,
    context: Context,
}

impl WideEvent {
    pub fn start(event: impl Into<String>) -> Self {
        Self {
            event: event.into(),
            started_at: Instant::now(),
            context: Context::new(),
        }
    }

    pub fn request_id(self, request_id: impl Into<String>) -> Self {
        self.with_str("request_id", request_id)
    }

    pub fn operation(self, operation: impl Into<String>) -> Self {
        self.with_str("operation", operation)
    }

    pub fn model(self, model: impl Into<String>) -> Self {
        self.with_str("model", model)
    }

    pub fn execution_mode(self, execution_mode: impl Into<String>) -> Self {
        self.with_str("execution_mode", execution_mode)
    }

    pub fn outcome(self, outcome: impl Into<String>) -> Self {
        self.with_str("outcome", outcome)
    }

    pub fn error_code(self, error_code: impl Into<String>) -> Self {
        self.with_str("error_code", error_code)
    }

    pub fn rows_scanned(self, value: u64) -> Self {
        self.with_u64("rows_scanned", value)
    }

    pub fn rows_returned(self, value: u64) -> Self {
        self.with_u64("rows_returned", value)
    }

    pub fn bytes_read(self, value: u64) -> Self {
        self.with_u64("bytes_read", value)
    }

    pub fn bytes_written(self, value: u64) -> Self {
        self.with_u64("bytes_written", value)
    }

    pub fn fallback_used(self, value: bool) -> Self {
        self.with_bool("fallback_used", value)
    }

    pub fn with_str(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context = self.context.with_str(key, value);
        self
    }

    pub fn with_u64(mut self, key: impl Into<String>, value: u64) -> Self {
        self.context = self.context.with_u64(key, value);
        self
    }

    pub fn with_bool(mut self, key: impl Into<String>, value: bool) -> Self {
        self.context = self.context.with_bool(key, value);
        self
    }

    pub fn into_context(self) -> Context {
        let duration_ms = self.started_at.elapsed().as_millis() as u64;
        self.context
            .with_str("wide_event", self.event)
            .with_u64("duration_ms", duration_ms)
    }

    pub fn finish_info(self) {
        let event = self.event.clone();
        crate::slog::info(&event, self.into_context());
    }

    pub fn finish_error(self) {
        let event = self.event.clone();
        crate::slog::error(&event, self.into_context());
    }
}

#[cfg(test)]
mod tests {
    use super::WideEvent;
    use serde_json::{Map, Value};

    #[test]
    fn wide_event_carries_completion_fields() {
        let context = WideEvent::start("query_complete")
            .request_id("req-1")
            .operation("search")
            .model("hybrid")
            .rows_scanned(10)
            .rows_returned(2)
            .outcome("success")
            .into_context();

        let mut fields = Map::new();
        context.merge_into(&mut fields);

        assert_eq!(fields.get("request_id"), Some(&Value::from("req-1")));
        assert_eq!(fields.get("operation"), Some(&Value::from("search")));
        assert_eq!(fields.get("model"), Some(&Value::from("hybrid")));
        assert_eq!(fields.get("rows_scanned"), Some(&Value::from(10)));
        assert_eq!(fields.get("rows_returned"), Some(&Value::from(2)));
        assert!(fields.contains_key("duration_ms"));
    }
}
