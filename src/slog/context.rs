use serde_json::{Map, Value};

#[derive(Clone, Debug, Default)]
pub struct Context {
    fields: Map<String, Value>,
}

impl Context {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    pub fn with_str(self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.with_field(key, Value::String(value.into()))
    }

    pub fn with_u64(self, key: impl Into<String>, value: u64) -> Self {
        self.with_field(key, Value::from(value))
    }

    pub fn with_i64(self, key: impl Into<String>, value: i64) -> Self {
        self.with_field(key, Value::from(value))
    }

    pub fn with_bool(self, key: impl Into<String>, value: bool) -> Self {
        self.with_field(key, Value::from(value))
    }

    pub fn merge_into(self, target: &mut Map<String, Value>) {
        for (key, value) in self.fields {
            target.insert(key, value);
        }
    }
}
