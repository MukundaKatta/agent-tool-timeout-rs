/*!
`agent-tool-timeout`: per-tool deadline enforcement for LLM agent tool calls.

When an LLM agent invokes external tools (shell commands, HTTP requests, database
queries, sub-agents, ...) some of those calls can hang or run far longer than is
useful. This crate provides a small, dependency-free [`TimeoutRegistry`] that lets
you configure a default deadline plus optional per-tool overrides, and then check
whether a given call exceeded its budget.

The crate does *not* spawn threads or cancel work for you — it is a pure
bookkeeping layer over durations, which keeps it usable from sync code, async
runtimes, and `no_std`-adjacent embedding alike. You decide how to measure
elapsed time (or use the convenience [`TimeoutRegistry::measure`] /
[`TimeoutRegistry::guard`] helpers) and how to react to a [`ToolTimeout`].

# Quick start

```rust
use agent_tool_timeout::TimeoutRegistry;

let mut reg = TimeoutRegistry::new(30_000); // 30s default
reg.set_timeout("slow_tool", 60_000);

assert_eq!(reg.timeout_for("slow_tool"), 60_000);
assert_eq!(reg.timeout_for("any_other"), 30_000);

// A call that finished within budget:
assert!(reg.check_elapsed("any_other", 12_000).is_ok());

// A call that blew past its budget:
let err = reg.check_elapsed("any_other", 45_000).unwrap_err();
assert_eq!(err.tool_name, "any_other");
assert_eq!(err.limit_ms, 30_000);
```

# Timing a closure

Use [`measure`](TimeoutRegistry::measure) to run a synchronous closure, time it
with [`std::time::Instant`], and report a [`ToolTimeout`] if it ran over budget.
The closure's return value is preserved on success:

```rust
use agent_tool_timeout::TimeoutRegistry;

let mut reg = TimeoutRegistry::new(1_000);
let value = reg.measure("compute", || 2 + 2).expect("fast enough");
assert_eq!(value, 4);
```
*/

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::fmt;
use std::time::Instant;

/// Error returned when a tool call exceeds its configured deadline.
///
/// Carries the offending tool name together with the measured `elapsed_ms` and
/// the `limit_ms` that was breached, so callers can log or surface a precise
/// message. Implements [`std::error::Error`] and [`std::fmt::Display`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTimeout {
    /// Name of the tool whose call exceeded its deadline.
    pub tool_name: String,
    /// Wall-clock time the call actually took, in milliseconds.
    pub elapsed_ms: u64,
    /// Deadline that was exceeded, in milliseconds.
    pub limit_ms: u64,
}

impl ToolTimeout {
    /// Milliseconds the call ran past its limit (`elapsed_ms - limit_ms`).
    ///
    /// Saturating subtraction: returns `0` if the call was not actually over
    /// budget, so this never panics or underflows.
    pub fn overrun_ms(&self) -> u64 {
        self.elapsed_ms.saturating_sub(self.limit_ms)
    }
}

impl fmt::Display for ToolTimeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "tool '{}' timed out: {}ms elapsed, limit {}ms",
            self.tool_name, self.elapsed_ms, self.limit_ms
        )
    }
}

impl std::error::Error for ToolTimeout {}

/// Registry of per-tool timeout limits with a shared default.
///
/// Construct with [`TimeoutRegistry::new`], then optionally register per-tool
/// overrides with [`set_timeout`](TimeoutRegistry::set_timeout). Query the
/// effective limit with [`timeout_for`](TimeoutRegistry::timeout_for) or enforce
/// it directly with [`check_elapsed`](TimeoutRegistry::check_elapsed),
/// [`measure`](TimeoutRegistry::measure), or [`guard`](TimeoutRegistry::guard).
///
/// # Examples
///
/// ```
/// use agent_tool_timeout::TimeoutRegistry;
///
/// let mut reg = TimeoutRegistry::new(5_000);
/// reg.set_timeout("db_query", 10_000);
/// assert_eq!(reg.timeout_for("db_query"), 10_000);
/// assert_eq!(reg.timeout_for("http_get"), 5_000);
/// ```
#[derive(Debug, Clone)]
pub struct TimeoutRegistry {
    default_ms: u64,
    per_tool: HashMap<String, u64>,
}

impl TimeoutRegistry {
    /// Create a registry with the given default timeout (in milliseconds).
    ///
    /// The default applies to every tool that does not have an explicit
    /// override registered via [`set_timeout`](Self::set_timeout).
    pub fn new(default_ms: u64) -> Self {
        Self {
            default_ms,
            per_tool: HashMap::new(),
        }
    }

    /// Register or replace the timeout (in milliseconds) for a specific tool.
    ///
    /// Overrides take precedence over the default for that tool name only.
    pub fn set_timeout(&mut self, tool: &str, ms: u64) {
        self.per_tool.insert(tool.to_string(), ms);
    }

    /// Return the effective timeout for `tool`: its override if one exists,
    /// otherwise the registry default.
    pub fn timeout_for(&self, tool: &str) -> u64 {
        *self.per_tool.get(tool).unwrap_or(&self.default_ms)
    }

    /// Return `true` if `tool` has an explicit override (not just the default).
    pub fn has_override(&self, tool: &str) -> bool {
        self.per_tool.contains_key(tool)
    }

    /// Check whether `elapsed_ms` exceeded the effective limit for `tool`.
    ///
    /// Returns `Ok(())` when `elapsed_ms <= limit` and `Err(ToolTimeout)` when
    /// the call ran strictly past its budget. A call that finishes *exactly* at
    /// the limit is considered on time.
    ///
    /// ```
    /// use agent_tool_timeout::TimeoutRegistry;
    /// let reg = TimeoutRegistry::new(1_000);
    /// assert!(reg.check_elapsed("t", 1_000).is_ok());  // exactly at limit
    /// assert!(reg.check_elapsed("t", 1_001).is_err()); // one ms over
    /// ```
    pub fn check_elapsed(&self, tool: &str, elapsed_ms: u64) -> Result<(), ToolTimeout> {
        let limit = self.timeout_for(tool);
        if elapsed_ms > limit {
            Err(ToolTimeout {
                tool_name: tool.to_string(),
                elapsed_ms,
                limit_ms: limit,
            })
        } else {
            Ok(())
        }
    }

    /// Run a synchronous closure, time it, and enforce the tool's deadline.
    ///
    /// On success the closure's return value is forwarded unchanged. If the
    /// closure runs longer than the tool's effective limit, the value is
    /// dropped and a [`ToolTimeout`] is returned instead. Note this does *not*
    /// cancel the closure — it can only detect an overrun after the closure
    /// returns.
    ///
    /// ```
    /// use agent_tool_timeout::TimeoutRegistry;
    /// let reg = TimeoutRegistry::new(60_000);
    /// let n = reg.measure("add", || 40 + 2).unwrap();
    /// assert_eq!(n, 42);
    /// ```
    pub fn measure<T, F: FnOnce() -> T>(&self, tool: &str, f: F) -> Result<T, ToolTimeout> {
        let start = Instant::now();
        let value = f();
        let elapsed_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
        self.check_elapsed(tool, elapsed_ms)?;
        Ok(value)
    }

    /// Start a [`Guard`] that records the start time for a tool call.
    ///
    /// Call [`Guard::check`] when the work finishes to enforce the deadline.
    /// This is convenient when the timed work spans code that cannot be wrapped
    /// in a single closure (e.g. async `.await` points).
    ///
    /// ```
    /// use agent_tool_timeout::TimeoutRegistry;
    /// let reg = TimeoutRegistry::new(10_000);
    /// let guard = reg.guard("io");
    /// // ... do work ...
    /// assert!(guard.check().is_ok());
    /// ```
    pub fn guard<'a>(&'a self, tool: &str) -> Guard<'a> {
        Guard {
            registry: self,
            tool: tool.to_string(),
            start: Instant::now(),
        }
    }

    /// The registry-wide default timeout in milliseconds.
    pub fn default_ms(&self) -> u64 {
        self.default_ms
    }

    /// Replace the registry-wide default timeout in milliseconds.
    ///
    /// Existing per-tool overrides are unaffected.
    pub fn set_default_ms(&mut self, default_ms: u64) {
        self.default_ms = default_ms;
    }

    /// All registered per-tool overrides, sorted by tool name.
    ///
    /// The default is not included — only explicit overrides appear here.
    pub fn all_timeouts(&self) -> Vec<(&str, u64)> {
        let mut v: Vec<(&str, u64)> = self
            .per_tool
            .iter()
            .map(|(k, &v)| (k.as_str(), v))
            .collect();
        v.sort_by_key(|(k, _)| *k);
        v
    }

    /// Number of registered per-tool overrides.
    pub fn len(&self) -> usize {
        self.per_tool.len()
    }

    /// Return `true` if no per-tool overrides are registered.
    pub fn is_empty(&self) -> bool {
        self.per_tool.is_empty()
    }

    /// Remove the override for `tool`, reverting it to the default.
    ///
    /// Returns the previous override value if one was set.
    pub fn clear_timeout(&mut self, tool: &str) -> Option<u64> {
        self.per_tool.remove(tool)
    }
}

/// RAII-style timer returned by [`TimeoutRegistry::guard`].
///
/// Records the start instant when created; call [`check`](Guard::check) (or
/// [`elapsed_ms`](Guard::elapsed_ms)) once the work is done to evaluate the
/// deadline against the originating registry.
#[derive(Debug)]
pub struct Guard<'a> {
    registry: &'a TimeoutRegistry,
    tool: String,
    start: Instant,
}

impl Guard<'_> {
    /// Milliseconds elapsed since the guard was created.
    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis().min(u64::MAX as u128) as u64
    }

    /// Check the elapsed time so far against the tool's deadline.
    ///
    /// Returns `Err(ToolTimeout)` if the work has already exceeded its budget.
    /// The guard can be checked multiple times.
    pub fn check(&self) -> Result<(), ToolTimeout> {
        self.registry.check_elapsed(&self.tool, self.elapsed_ms())
    }
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
        let reg = TimeoutRegistry::new(30_000);
        assert!(reg.check_elapsed("tool", 10_000).is_ok());
    }

    #[test]
    fn check_elapsed_err() {
        let reg = TimeoutRegistry::new(30_000);
        assert!(reg.check_elapsed("tool", 50_000).is_err());
    }

    #[test]
    fn check_elapsed_at_limit_ok() {
        let reg = TimeoutRegistry::new(30_000);
        assert!(reg.check_elapsed("tool", 30_000).is_ok()); // exactly at limit = ok
    }

    #[test]
    fn check_elapsed_over_limit_err() {
        let reg = TimeoutRegistry::new(30_000);
        assert!(reg.check_elapsed("tool", 30_001).is_err()); // one ms over
    }

    #[test]
    fn error_fields_and_display() {
        let reg = TimeoutRegistry::new(3_000);
        let err = reg.check_elapsed("fn", 5_000).unwrap_err();
        assert_eq!(err.tool_name, "fn");
        assert_eq!(err.elapsed_ms, 5_000);
        assert_eq!(err.limit_ms, 3_000);
        let msg = err.to_string();
        assert!(msg.contains("fn"));
        assert!(msg.contains("5000"));
        assert!(msg.contains("3000"));
    }

    #[test]
    fn overrun_ms_reports_excess() {
        let err = ToolTimeout {
            tool_name: "t".into(),
            elapsed_ms: 5_000,
            limit_ms: 3_000,
        };
        assert_eq!(err.overrun_ms(), 2_000);
    }

    #[test]
    fn overrun_ms_saturates_to_zero() {
        let err = ToolTimeout {
            tool_name: "t".into(),
            elapsed_ms: 1_000,
            limit_ms: 3_000,
        };
        assert_eq!(err.overrun_ms(), 0);
    }

    #[test]
    fn clear_timeout_reverts_to_default_and_returns_previous() {
        let mut reg = TimeoutRegistry::new(30_000);
        reg.set_timeout("t", 5_000);
        assert_eq!(reg.clear_timeout("t"), Some(5_000));
        assert_eq!(reg.timeout_for("t"), 30_000);
        assert_eq!(reg.clear_timeout("t"), None);
    }

    #[test]
    fn all_timeouts_sorted() {
        let mut reg = TimeoutRegistry::new(1_000);
        reg.set_timeout("z", 9_000);
        reg.set_timeout("a", 2_000);
        let v = reg.all_timeouts();
        assert_eq!(v, vec![("a", 2_000), ("z", 9_000)]);
    }

    #[test]
    fn default_ms_getter_and_setter() {
        let mut reg = TimeoutRegistry::new(5_000);
        assert_eq!(reg.default_ms(), 5_000);
        reg.set_default_ms(8_000);
        assert_eq!(reg.default_ms(), 8_000);
        assert_eq!(reg.timeout_for("x"), 8_000);
    }

    #[test]
    fn per_tool_with_check() {
        let mut reg = TimeoutRegistry::new(30_000);
        reg.set_timeout("fast", 1_000);
        assert!(reg.check_elapsed("fast", 2_000).is_err());
        assert!(reg.check_elapsed("fast", 500).is_ok());
    }

    #[test]
    fn has_override_and_len_and_empty() {
        let mut reg = TimeoutRegistry::new(1_000);
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(!reg.has_override("t"));
        reg.set_timeout("t", 2_000);
        assert!(reg.has_override("t"));
        assert!(!reg.is_empty());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn measure_forwards_value_when_fast() {
        let reg = TimeoutRegistry::new(60_000);
        let v = reg.measure("compute", || 21 * 2).unwrap();
        assert_eq!(v, 42);
    }

    #[test]
    fn measure_errors_when_over_budget() {
        // Zero-millisecond budget: any measurable work is over budget, but to
        // make the test deterministic we use a closure that sleeps past 0ms.
        let reg = TimeoutRegistry::new(0);
        let result = reg.measure("sleepy", || {
            std::thread::sleep(std::time::Duration::from_millis(5));
        });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().limit_ms, 0);
    }

    #[test]
    fn guard_within_budget() {
        let reg = TimeoutRegistry::new(60_000);
        let g = reg.guard("io");
        assert!(g.check().is_ok());
        assert!(g.elapsed_ms() < 60_000);
    }

    #[test]
    fn guard_over_budget() {
        let reg = TimeoutRegistry::new(0);
        let g = reg.guard("io");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let err = g.check().unwrap_err();
        assert_eq!(err.tool_name, "io");
        assert_eq!(err.limit_ms, 0);
    }

    #[test]
    fn registry_is_cloneable() {
        let mut reg = TimeoutRegistry::new(1_000);
        reg.set_timeout("t", 2_000);
        let clone = reg.clone();
        assert_eq!(clone.timeout_for("t"), 2_000);
        assert_eq!(clone.default_ms(), 1_000);
    }
}
