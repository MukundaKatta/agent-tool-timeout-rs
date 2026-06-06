# agent-tool-timeout

Per-tool deadline enforcement for LLM agent tool calls.

When an LLM agent invokes external tools (shell commands, HTTP requests, database
queries, and so on), a single slow or hung tool can stall the whole agent loop.
`agent-tool-timeout` provides a small, dependency-free `TimeoutRegistry` that lets
you declare a default deadline and override it per tool, then check whether a given
tool call exceeded its limit.

## Features

- Configurable default timeout applied to every tool.
- Per-tool overrides, with a clean fallback to the default.
- Explicit `check_elapsed` API that returns a typed `ToolTimeout` error when a call
  runs past its deadline (calls exactly at the limit are allowed; only strictly
  greater elapsed times fail).
- An internal elapsed-time log useful for testing and introspection.
- No external dependencies — pure Rust standard library.

## Installation

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
agent-tool-timeout = "0.1"
```

## Usage

```rust
use agent_tool_timeout::TimeoutRegistry;

// Default deadline of 30 seconds for any tool.
let mut reg = TimeoutRegistry::new(30_000);

// Give a known-slow tool a longer budget.
reg.set_timeout("slow_tool", 60_000);

assert_eq!(reg.timeout_for("slow_tool"), 60_000);
assert_eq!(reg.timeout_for("any_other"), 30_000);

// After running a tool, check whether it exceeded its deadline.
match reg.check_elapsed("slow_tool", 45_000) {
    Ok(()) => println!("within budget"),
    Err(timeout) => eprintln!("{timeout}"),
}
```

### API overview

| Method | Description |
| --- | --- |
| `TimeoutRegistry::new(default_ms)` | Create a registry with a default timeout in milliseconds. |
| `set_timeout(tool, ms)` | Override the timeout for a specific tool. |
| `timeout_for(tool)` | Resolve the effective timeout for a tool (override or default). |
| `check_elapsed(tool, elapsed_ms)` | Return `Err(ToolTimeout)` if `elapsed_ms` exceeds the tool's limit. |
| `clear_timeout(tool)` | Remove a per-tool override so it falls back to the default. |
| `all_timeouts()` | List all per-tool overrides, sorted by tool name. |
| `default_ms()` | Read the configured default timeout. |
| `elapsed_log()` | Inspect the recorded `(tool, elapsed_ms)` history. |

On timeout, `ToolTimeout` carries the `tool_name`, `elapsed_ms`, and `limit_ms`,
implements `std::error::Error`, and renders a readable message such as:

```text
tool 'slow_tool' timed out: 65000ms elapsed, limit 60000ms
```

## Building and testing

```sh
cargo build
cargo test
```

## Tech stack

- Language: Rust (2021 edition)
- Dependencies: none (standard library only)

## License

Licensed under the MIT License.
