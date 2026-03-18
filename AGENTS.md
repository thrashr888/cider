# Repository Guidelines

## Project Structure & Module Organization
`src/main.rs` defines the Clap CLI and dispatches commands. `src/sources/*.rs` holds one module per Apple app or data source, such as `reminders.rs` or `calendar.rs`; keep new integrations in snake_case and export them from `src/sources/mod.rs`. `src/sources/util.rs` contains shared subprocess, timeout, and date helpers. `src/pretty.rs` owns human-readable `--pretty` rendering. Repo-specific agent skills live in `.agents/skills/`, and the end-user CLI skill lives in `.skills/`. `.github/workflows/` contains CI and release automation. `target/` is generated build output and should stay untracked.

## Build, Test, and Development Commands
Use `cargo run -- reminders --pretty` to exercise a single command locally. Run `cargo build --release` to produce the binary at `target/release/cider`. Run `cargo test` for the repository’s inline unit tests. Before opening a PR, match CI with `cargo fmt -- --check` and `cargo clippy -- -D warnings`. CI runs on `macos-latest`, so macOS-specific behavior should be verified there.

## Coding Style & Naming Conventions
Follow `rustfmt` defaults: 4-space indentation, standard import grouping, and formatter-controlled line wrapping. Use `snake_case` for files, modules, functions, and JSON fields; use `UpperCamelCase` for structs and enums. Prefer small source-specific modules and reuse helpers from `src/sources/util.rs` instead of duplicating AppleScript, JXA, or timeout logic. Keep stdout machine-readable JSON and send diagnostics to stderr; any `--pretty` presentation logic belongs in `src/pretty.rs`.

## Testing Guidelines
This repo keeps tests inline with implementation under `#[cfg(test)] mod tests` rather than a separate `tests/` directory. Add deterministic unit tests for parsing, formatting, schema changes, and action result shapes. Avoid tests that require live Apple app state unless there is no stable alternative. When changing output, cover both the JSON contract and any affected pretty rendering.

## Commit & Pull Request Guidelines
Recent history uses short, imperative commit subjects, often with Conventional Commit prefixes like `feat:` and `fix:`; release commits use `Bump to vX.Y.Z`. Keep commits focused and easy to scan. PRs should explain which commands or source modules changed, list validation performed (`cargo test`, `cargo clippy`, manual command runs), and include sample command/output snippets for user-facing CLI changes. Call out any macOS permission or side-effect implications for mutating commands.

## Release Process
Releases are automated via `.github/workflows/release.yaml`, triggered by pushing a `v*` tag. The workflow builds macOS binaries (aarch64 + x86_64), creates a GitHub release with tarballs, publishes to crates.io, and updates the Homebrew tap.

To cut a release:
1. Bump `version` in `Cargo.toml` and commit: `Bump to vX.Y.Z`
2. Push the commit to `main`
3. Run `gh release create vX.Y.Z --target main --draft --generate-notes`
4. The workflow triggers on the tag push, builds macOS binaries, uploads them to the draft release, publishes it, pushes to crates.io, and updates the Homebrew tap

Note: Do not push tags directly (`git push origin vX.Y.Z`) — repository rulesets block it. Use `gh release create --draft` which creates the tag through the Releases API and leaves the release as a draft for the workflow to finalize.
