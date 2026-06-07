# agent-tool-timeout

[![CI](https://github.com/MukundaKatta/agent-tool-timeout-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/MukundaKatta/agent-tool-timeout-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Per-tool deadline enforcement for LLM agent tool calls.

When an LLM agent invokes external tools — shell commands, HTTP requests,
database queries, sub-agents — some of those calls hang or run far longer than is
useful. `agent-tool-timeout` is a small, **dependency-free** bookkeeping layer
that lets you configure a default deadline plus optional per-tool overrides and
then check whether any given call exceeded its budget.

## What it is (and isn't)

- It **is** a pure, allocation-light registry of durations plus convenience
  helpers for timing synchronous work.
- It **does not** spawn threads, cancel work, or impose an async runtime. You
  decide how to measure elapsed time and how to react to a timeout. This keeps
  it usable from sync code, any async runtime, and embedding scenarios alike.

## Install

Add it to your `Cargo.toml`:

```toml
[dependencies]
agent-tool-timeout = { git = "https://github.com/MukundaKatta/agent-tool-timeout-rs" }
```

The crate has **zero runtime dependencies** and builds on stable Rust
(edition 2021).

## Usage

### Configure budgets and check a call

```rust
use agent_tool_timeout::TimeoutRegistry;

let mut reg = TimeoutRegistry::new(30_000); // 30s default
reg.set_timeout("slow_tool", 60_000);       // per-tool override

assert_eq!(reg.timeout_for("slow_tool"), 60_000);
assert_eq!(reg.timeout_for("any_other"), 30_000); // falls back to default

// A call that finished within budget:
assert!(reg.check_elapsed("any_other", 12_000).is_ok());

// A call that blew past its budget:
let err = reg.check_elapsed("any_other", 45_000).unwrap_err();
assert_eq!(err.tool_name, "any_other");
assert_eq!(err.limit_ms, 30_000);
assert_eq!(err.overrun_ms(), 15_000);
println!("{err}"); // tool 'any_other' timed out: 45000ms elapsed, limit 30000ms
```

### Time a synchronous closure

`measure` runs a closure, times it with `std::time::Instant`, and returns either
the closure's value or a `ToolTimeout`:

```rust
use agent_tool_timeout::TimeoutRegistry;

let reg = TimeoutRegistry::new(1_000);
let value = reg.measure("compute", || 2 + 2)?;
assert_eq!(value, 4);
# Ok::<(), agent_tool_timeout::ToolTimeout>(())
```

### Time work that spans multiple statements (e.g. async)

When the timed work can't be wrapped in a single closure — for instance because
it contains `.await` points — start a `Guard` and check it when done:

```rust
use agent_tool_timeout::TimeoutRegistry;

let reg = TimeoutRegistry::new(10_000);
let guard = reg.guard("network_call");

// ... perform the work, possibly across several .await points ...

guard.check()?; // Err(ToolTimeout) if the deadline was already exceeded
# Ok::<(), agent_tool_timeout::ToolTimeout>(())
```

> Note: `measure` and `guard` detect overruns *after* the work returns — they do
> not interrupt or cancel in-flight work. To hard-cancel, combine `check_elapsed`
> with your runtime's cancellation primitives (e.g. `tokio::time::timeout`).

## API overview

### `TimeoutRegistry`

| Method | Description |
| --- | --- |
| `new(default_ms)` | Create a registry with a default timeout. |
| `set_timeout(tool, ms)` | Register/replace a per-tool override. |
| `timeout_for(tool) -> u64` | Effective limit (override or default). |
| `has_override(tool) -> bool` | Whether `tool` has an explicit override. |
| `check_elapsed(tool, elapsed_ms) -> Result<(), ToolTimeout>` | Compare a measured duration to the limit. |
| `measure(tool, closure) -> Result<T, ToolTimeout>` | Time a closure and enforce the deadline. |
| `guard(tool) -> Guard` | Start a timer; call `Guard::check()` later. |
| `default_ms() / set_default_ms(ms)` | Get/replace the default timeout. |
| `all_timeouts() -> Vec<(&str, u64)>` | All overrides, sorted by tool name. |
| `len() / is_empty()` | Number of overrides. |
| `clear_timeout(tool) -> Option<u64>` | Remove an override, returning its old value. |

### `Guard<'a>`

| Method | Description |
| --- | --- |
| `elapsed_ms() -> u64` | Milliseconds since the guard was created. |
| `check() -> Result<(), ToolTimeout>` | Enforce the deadline against elapsed time. |

### `ToolTimeout`

An `Error` carrying `tool_name`, `elapsed_ms`, and `limit_ms`, plus
`overrun_ms()` (saturating `elapsed_ms - limit_ms`). Implements `Display`,
`Clone`, and `PartialEq`.

## Semantics

- A call that finishes **exactly at** the limit is considered on time;
  `check_elapsed` only errors when `elapsed_ms > limit_ms`.
- The default applies to every tool without an explicit override.
- The registry is `Clone` and holds no global state.

## Development

```sh
cargo build
cargo test          # unit + integration + doc tests
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

## License

Licensed under the [MIT License](LICENSE).
