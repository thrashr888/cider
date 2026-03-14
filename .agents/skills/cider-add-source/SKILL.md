---
name: cider-add-source
description: Add or extend a cider CLI source module. Use when adding a new Apple app/data source, a new subcommand, or new read/write behavior that must be wired through src/main.rs, src/sources/mod.rs, tests, and docs.
---

# Cider Add Source

Read these files first:

- `src/main.rs`
- `src/sources/mod.rs`
- The target module in `src/sources/`
- `src/sources/util.rs`
- `README.md` only if the CLI surface changes

## Workflow

1. Pick the access pattern that matches nearby code before inventing a new one:
   - `run_command_with_timeout` for shell tools such as `sqlite3`, `defaults`, `mdfind`
   - `run_jxa` / `run_jxa_with_timeout` for JavaScript automation
   - `run_osascript_with_timeout` for AppleScript
2. Keep source modules self-contained. New files belong in `src/sources/<name>.rs` and must be exported from `src/sources/mod.rs`.
3. In `src/main.rs`, add the Clap command shape, then wire read operations through `run_source!` and write operations through `print_output`.
4. Support `--dry-run` for mutating commands in the CLI branch. The source function should perform the real mutation; dry-run stays in `src/main.rs`.
5. If the command is discoverable through `schema`, update `build_schema` with `supports_dry_run`, pagination args, and `stable_ids` when relevant.

## Data Shape

- Return typed `serde::Serialize` structs, not ad hoc JSON.
- Use `snake_case` field names.
- Mark optional fields with `#[serde(skip_serializing_if = "Option::is_none")]`.
- Mutations should return `ActionResult` from `src/sources/util.rs`.
- Keep stdout machine-readable JSON. Use stderr for warnings like skipped databases or missing permissions.

## Tests And Validation

- Prefer inline `#[cfg(test)] mod tests` in the module you changed.
- Add deterministic tests for parsers, output shaping, and escaping helpers.
- Avoid tests that depend on live app state unless there is no stable alternative.
- Run `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test`, and at least one representative `cargo run -- <command>`.
