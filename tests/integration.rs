//! Integration tests exercising the public API of `agent-tool-timeout`
//! exactly as a downstream crate would consume it.

use agent_tool_timeout::{TimeoutRegistry, ToolTimeout};
use std::error::Error;

#[test]
fn end_to_end_default_and_override() {
    let mut reg = TimeoutRegistry::new(30_000);
    reg.set_timeout("slow_tool", 60_000);
    reg.set_timeout("fast_tool", 1_000);

    // Defaults apply to unknown tools.
    assert_eq!(reg.timeout_for("unknown"), 30_000);
    assert!(reg.check_elapsed("unknown", 30_000).is_ok());
    assert!(reg.check_elapsed("unknown", 30_001).is_err());

    // Overrides take precedence.
    assert_eq!(reg.timeout_for("slow_tool"), 60_000);
    assert!(reg.check_elapsed("slow_tool", 59_999).is_ok());
    assert!(reg.check_elapsed("fast_tool", 1_500).is_err());
}

#[test]
fn timeout_error_is_a_std_error() {
    let reg = TimeoutRegistry::new(100);
    let err: ToolTimeout = reg.check_elapsed("io", 250).unwrap_err();

    // Usable as a boxed std::error::Error.
    let boxed: Box<dyn Error> = Box::new(err.clone());
    assert!(boxed.to_string().contains("io"));

    assert_eq!(err.overrun_ms(), 150);
}

#[test]
fn measure_returns_value_and_enforces_budget() {
    let reg = TimeoutRegistry::new(60_000);

    let sum: i32 = reg.measure("sum", || (1..=10).sum()).unwrap();
    assert_eq!(sum, 55);

    // A deliberately slow call against a zero budget overruns.
    let reg_zero = TimeoutRegistry::new(0);
    let outcome = reg_zero.measure("slow", || {
        std::thread::sleep(std::time::Duration::from_millis(5));
        "done"
    });
    assert!(outcome.is_err());
}

#[test]
fn guard_tracks_elapsed_time() {
    let reg = TimeoutRegistry::new(60_000);
    let guard = reg.guard("network");
    assert!(guard.check().is_ok());
    // Elapsed time is monotonic and within the budget for trivial work.
    assert!(guard.elapsed_ms() < 60_000);
}

#[test]
fn introspection_helpers() {
    let mut reg = TimeoutRegistry::new(5_000);
    assert!(reg.is_empty());

    reg.set_timeout("b", 2_000);
    reg.set_timeout("a", 1_000);
    assert_eq!(reg.len(), 2);
    assert!(reg.has_override("a"));
    assert_eq!(reg.all_timeouts(), vec![("a", 1_000), ("b", 2_000)]);

    assert_eq!(reg.clear_timeout("a"), Some(1_000));
    assert_eq!(reg.timeout_for("a"), 5_000);
    assert!(!reg.has_override("a"));
}
