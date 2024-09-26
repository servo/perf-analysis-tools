use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Chromium JSON trace.
///
/// Format spec: <https://docs.google.com/document/d/1CvAClvFfyA5R-PhYUmn5OOQtYMH4h6I0nSsKchNAySU>
#[derive(Clone, Debug, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct JsonTrace {
    pub traceEvents: Vec<TraceEvent>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct TraceEvent {
    pub ts: usize,
    pub dur: Option<usize>,
    pub ph: String,
    pub s: Option<String>,
    pub name: String,
    pub cat: String,
    pub pid: usize,
    pub tid: usize,
    pub args: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub _rest: BTreeMap<String, Value>,
}
