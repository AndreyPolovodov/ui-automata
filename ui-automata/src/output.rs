use std::collections::HashMap;

/// Workflow output buffer. Populated by `Extract` actions during execution
/// and returned to the caller after the workflow completes.
///
/// Each key maps to an ordered list of extracted string values. A key written
/// by a non-`multiple` Extract will have exactly one entry; a `multiple` Extract
/// appends all matched values in document order.
#[derive(Debug, Clone, Default)]
pub struct Output {
    data: HashMap<String, Vec<String>>,
}

impl Output {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a value under `key`.
    pub fn push(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.data.entry(key.into()).or_default().push(value.into());
    }

    /// All values stored under `key`, or an empty slice if the key was never written.
    pub fn get(&self, key: &str) -> &[String] {
        self.data.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Consume and return the underlying map.
    pub fn into_map(self) -> HashMap<String, Vec<String>> {
        self.data
    }

    /// Reference to the underlying map.
    pub fn as_map(&self) -> &HashMap<String, Vec<String>> {
        &self.data
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Merge all key/value entries from `other` into this output.
    /// Values are appended in order under each key.
    pub fn merge(&mut self, other: Output) {
        for (key, values) in other.data {
            self.data.entry(key).or_default().extend(values);
        }
    }
}
