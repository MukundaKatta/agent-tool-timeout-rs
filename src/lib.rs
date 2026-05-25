/*!
agent-tool-timeout: per-tool deadline enforcement for LLM agent tool calls.

```rust
use agent_tool_timeout::TimeoutRegistry;

let mut reg = TimeoutRegistry::new(30_000);    // 30s default
reg.set_timeout("slow_tool", 60_000);
assert_eq!(reg.timeout_for("slow_tool"), 60_000);
assert_eq!(reg.timeout_for("any_other"), 30_000);
```
*/

use std::collections::HashMap;
use std::fmt;

/// Raised when a tool call exceeds its deadline.
#[derive(Debug)]
pub struct ToolTimeout {
    pub tool_name: String,
    pub elapsed_ms: u64,
    pub limit_ms: u64,
}

impl fmt::Display for ToolTimeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tool '{}' timed out: {}ms elapsed, limit {}ms", self.tool_name, self.elapsed_ms, self.limit_ms)
    }
}

impl std::error::Error for ToolTimeout {}

/// Manages per-tool timeout limits.
pub struct TimeoutRegistry {
    default_ms: u64,
    per_tool: HashMap<String, u64>,
    /// For testing: track calls manually via check_elapsed.
    elapsed_log: Vec<(String, u64)>,
}

impl TimeoutRegistry {
    /// Create with a default timeout.
    pub fn new(default_ms: u64) -> Self {
        Self { default_ms, per_tool: HashMap::new(), elapsed_log: Vec::new() }
    }

    /// Override timeout for a specific tool.
    pub fn set_timeout(&mut self, tool: &str, ms: u64) {
        self.per_tool.insert(tool.to_string(), ms);
    }

    /// Get timeout for a tool (per-tool or default).
    pub fn timeout_for(&self, tool: &str) -> u64 {
        *self.per_tool.get(tool).unwrap_or(&self.default_ms)
    }

    /// Check if `elapsed_ms` exceeds the tool's limit.
    pub fn check_elapsed(&mut self, tool: &str, elapsed_ms: u64) -> Result<(), ToolTimeout> {
        self.elapsed_log.push((tool.to_string(), elapsed_ms));
        let limit = self.timeout_for(tool);
        if elapsed_ms > limit {
            Err(ToolTimeout { tool_name: tool.to_string(), elapsed_ms, limit_ms: limit })
        } else {
            Ok(())
        }
    }

    pub fn default_ms(&self) -> u64 { self.default_ms }

    /// All tool-specific timeouts (sorted by name).
    pub fn all_timeouts(&self) -> Vec<(&str, u64)> {
        let mut v: Vec<(&str, u64)> = self.per_tool.iter().map(|(k, &v)| (k.as_str(), v)).collect();
        v.sort_by_key(|(k, _)| *k);
        v
    }

    /// Remove a per-tool override (falls back to default).
    pub fn clear_timeout(&mut self, tool: &str) { self.per_tool.remove(tool); }

    /// Check log entries.
    pub fn elapsed_log(&self) -> &[(String, u64)] { &self.elapsed_log }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeout_used() {
        let reg = TimeoutRegistry::new(30_000);
        assert_eq!(reg.timeout_for("any_tool"), 30_000);
    }

    #[test]
    fn per_tool_override() {
        let mut reg = TimeoutRegistry::new(30_000);
        reg.set_timeout("slow", 120_000);
        assert_eq!(reg.timeout_for("slow"), 120_000);
        assert_eq!(reg.timeout_for("other"), 30_000);
    }

    #[test]
    fn check_elapsed_ok() {
        let mut reg = TimeoutRegistry::new(30_000);
        assert!(reg.check_elapsed("tool", 10_000).is_ok());
    }

    #[test]
    fn check_elapsed_err() {
        let mut reg = TimeoutRegistry::new(30_000);
        assert!(reg.check_elapsed("tool", 50_000).is_err());
    }

    #[test]
    fn check_elapsed_at_limit_ok() {
        let mut reg = TimeoutRegistry::new(30_000);
        assert!(reg.check_elapsed("tool", 30_000).is_ok()); // exactly at limit = ok
    }

    #[test]
    fn check_elapsed_over_limit_err() {
        let mut reg = TimeoutRegistry::new(30_000);
        assert!(reg.check_elapsed("tool", 30_001).is_err()); // one ms over
    }

    #[test]
    fn error_display() {
        let e = ToolTimeout { tool_name: "fn".into(), elapsed_ms: 5000, limit_ms: 3000 };
        assert!(e.to_string().contains("fn"));
        assert!(e.to_string().contains("5000"));
    }

    #[test]
    fn clear_timeout_reverts_to_default() {
        let mut reg = TimeoutRegistry::new(30_000);
        reg.set_timeout("t", 5_000);
        reg.clear_timeout("t");
        assert_eq!(reg.timeout_for("t"), 30_000);
    }

    #[test]
    fn all_timeouts_sorted() {
        let mut reg = TimeoutRegistry::new(1000);
        reg.set_timeout("z", 9000);
        reg.set_timeout("a", 2000);
        let v = reg.all_timeouts();
        assert_eq!(v[0].0, "a");
        assert_eq!(v[1].0, "z");
    }

    #[test]
    fn elapsed_log_recorded() {
        let mut reg = TimeoutRegistry::new(10_000);
        reg.check_elapsed("t1", 500).ok();
        reg.check_elapsed("t2", 200).ok();
        assert_eq!(reg.elapsed_log().len(), 2);
    }

    #[test]
    fn default_ms_getter() {
        let reg = TimeoutRegistry::new(5000);
        assert_eq!(reg.default_ms(), 5000);
    }

    #[test]
    fn per_tool_with_check() {
        let mut reg = TimeoutRegistry::new(30_000);
        reg.set_timeout("fast", 1_000);
        assert!(reg.check_elapsed("fast", 2_000).is_err());
        assert!(reg.check_elapsed("fast", 500).is_ok());
    }
}
