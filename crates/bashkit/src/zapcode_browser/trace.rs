//! Execution trace for debugging and observability.
//!
//! Captures a tree of spans covering parse → compile → execute → tool calls.
//! The trace is lightweight and always collected (sub-microsecond overhead).
//!
//! The `TraceSpan` shape is designed to map cleanly to OpenTelemetry spans
//! for future export to Jaeger, Langfuse, Datadog, etc.

use crate::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A single span in the execution trace.
///
/// Shaped to be OTel-compatible: each span has a name, timestamps,
/// status, key-value attributes, and children.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSpan {
    /// Span name (e.g. "parse", "compile", "execute", "tool_call", "suspend").
    pub name: String,
    /// When the span started (ms since Unix epoch).
    pub start_time_ms: u64,
    /// When the span ended (ms since Unix epoch). 0 if still open.
    pub end_time_ms: u64,
    /// Duration in microseconds.
    pub duration_us: u64,
    /// "ok" or "error".
    pub status: TraceStatus,
    /// Structured attributes. Keys use `zapcode.*` namespace.
    pub attributes: Vec<(String, String)>,
    /// Child spans.
    pub children: Vec<TraceSpan>,
}

/// Span status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TraceStatus {
    Ok,
    Error,
}

/// Builder for constructing trace spans with proper timing.
pub(crate) struct SpanBuilder {
    name: String,
    start_wall: u64,
    start_instant: Instant,
    attributes: Vec<(String, String)>,
    children: Vec<TraceSpan>,
}

impl SpanBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            start_wall: now_ms(),
            start_instant: Instant::now(),
            attributes: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn attr(mut self, key: &str, value: impl ToString) -> Self {
        self.attributes.push((key.to_string(), value.to_string()));
        self
    }

    pub fn set_attr(&mut self, key: &str, value: impl ToString) {
        self.attributes.push((key.to_string(), value.to_string()));
    }

    pub fn add_child(&mut self, child: TraceSpan) {
        self.children.push(child);
    }

    pub fn finish(self, status: TraceStatus) -> TraceSpan {
        let elapsed = self.start_instant.elapsed();
        TraceSpan {
            name: self.name,
            start_time_ms: self.start_wall,
            end_time_ms: self.start_wall + elapsed.as_millis() as u64,
            duration_us: elapsed.as_micros() as u64,
            status,
            attributes: self.attributes,
            children: self.children,
        }
    }

    pub fn finish_ok(self) -> TraceSpan {
        self.finish(TraceStatus::Ok)
    }

    pub fn finish_error(self, error: &str) -> TraceSpan {
        self.attr("zapcode.error", error).finish(TraceStatus::Error)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Execution trace covering a full run (parse + compile + execute).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub root: TraceSpan,
}

impl ExecutionTrace {
    /// Pretty-print the trace as a tree.
    pub fn print(&self) {
        print_span(&self.root, 0, true);
    }

    /// Format the trace as a tree string.
    pub fn to_string_pretty(&self) -> String {
        let mut buf = String::new();
        format_span(&self.root, 0, true, &mut buf);
        buf
    }
}

fn format_duration(us: u64) -> String {
    if us < 1000 {
        format!("{}µs", us)
    } else if us < 1_000_000 {
        format!("{:.1}ms", us as f64 / 1000.0)
    } else {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    }
}

fn format_span(span: &TraceSpan, depth: usize, is_last: bool, buf: &mut String) {
    let icon = match span.status {
        TraceStatus::Ok => "✓",
        TraceStatus::Error => "✗",
    };
    let duration = format_duration(span.duration_us);

    // Build prefix
    let prefix = if depth == 0 {
        String::new()
    } else {
        let connector = if is_last { "└─ " } else { "├─ " };
        let indent = "│  ".repeat(depth - 1);
        format!("{}{}", indent, connector)
    };

    buf.push_str(&format!("{}{} {} ({})", prefix, icon, span.name, duration));

    // Show key attributes inline
    for (k, v) in &span.attributes {
        if k == "zapcode.error" {
            buf.push_str(&format!(" error=\"{}\"", v));
        } else if k == "zapcode.tool.name" {
            buf.push_str(&format!(" {}", v));
        } else if k == "zapcode.tool.args" {
            buf.push_str(&format!("({})", v));
        } else if k == "zapcode.tool.result" {
            let display = if v.len() > 60 { &v[..57] } else { v };
            buf.push_str(&format!(" → {}", display));
            if v.len() > 60 {
                buf.push_str("...");
            }
        }
    }
    buf.push('\n');

    for (i, child) in span.children.iter().enumerate() {
        format_span(child, depth + 1, i == span.children.len() - 1, buf);
    }
}

fn print_span(span: &TraceSpan, depth: usize, is_last: bool) {
    let mut buf = String::new();
    format_span(span, depth, is_last, &mut buf);
    print!("{}", buf);
}
