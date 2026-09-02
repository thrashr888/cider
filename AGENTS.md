# Repository Guidelines

## Project Structure & Module Organization
`src/lib.rs` is the library root and `src/main.rs` is the Clap CLI over it — the binary is a front-end, and every command's real work lives in a `sources` function. Keep it that way: logic that lands in `main.rs` is logic a library consumer (Alchemy links this crate) cannot reach. Clap belongs to the `cli` feature, on by default for the binary and off for library consumers, so nothing under `src/sources/` may depend on it. `src/sources/*.rs` holds one module per Apple app or data source, such as `reminders.rs` or `calendar.rs`; keep new integrations in snake_case and export them from `src/sources/mod.rs`. `src/sources/util.rs` contains shared subprocess, timeout, and date helpers. `src/pretty.rs` owns human-readable `--pretty` rendering. Repo-specific agent skills live in `.agents/skills/`, and the end-user CLI skill lives in `.skills/`. `.github/workflows/` contains CI and release automation. `target/` is generated build output and should stay untracked.

## Build, Test, and Development Commands
Use `cargo run -- reminders --pretty` to exercise a single command locally. Run `cargo build --release` to produce the binary at `target/release/cider`. Run `cargo test` for the repository’s inline unit tests (they live on the lib target). Also build the way a library consumer does — `cargo clippy --lib --no-default-features -- -D warnings` — which is what catches a Clap dependency creeping into `src/sources/`. Before opening a PR, match CI with `cargo fmt -- --check` and `cargo clippy -- -D warnings`. CI runs on `macos-latest`, so macOS-specific behavior should be verified there.

## Coding Style & Naming Conventions
Follow `rustfmt` defaults: 4-space indentation, standard import grouping, and formatter-controlled line wrapping. Use `snake_case` for files, modules, functions, and JSON fields; use `UpperCamelCase` for structs and enums. Prefer small source-specific modules and reuse helpers from `src/sources/util.rs` instead of duplicating AppleScript, JXA, or timeout logic. Keep stdout machine-readable JSON and send diagnostics to stderr; any `--pretty` presentation logic belongs in `src/pretty.rs`.

## Testing Guidelines
This repo keeps unit tests inline with implementation under `#[cfg(test)] mod tests`. The one file under `tests/` is the live smoke suite, `tests/live.rs`: it runs every read-only command against the Mac it is on and fails if a command hangs, prints something other than JSON, exits non-zero without a typed error envelope, or returns an empty list for a store that `cider doctor` reports as present (the bug that let `find-my` ship returning `[]`). It is `#[ignore]`d because CI runners have no user data and a JXA call can open a permission dialog; run it yourself before a release with `cargo test --test live -- --ignored --nocapture`. It is read-only by construction: a guard refuses any subcommand outside a short allowlist of read verbs, so it can never create, change, or delete anything on the machine. Keep it that way when adding cases. Add deterministic unit tests for parsing, formatting, schema changes, and action result shapes. Avoid tests that require live Apple app state unless there is no stable alternative. When changing output, cover both the JSON contract and any affected pretty rendering.

## Commit & Pull Request Guidelines
Recent history uses short, imperative commit subjects, often with Conventional Commit prefixes like `feat:` and `fix:`; release commits use `Bump to vX.Y.Z`. Keep commits focused and easy to scan. PRs should explain which commands or source modules changed, list validation performed (`cargo test`, `cargo clippy`, manual command runs), and include sample command/output snippets for user-facing CLI changes. Call out any macOS permission or side-effect implications for mutating commands.

## Release Process
Releases are automated via `.github/workflows/release.yaml`, triggered when a `v*` tag appears. The workflow builds macOS binaries (aarch64 + x86_64), creates a GitHub release with tarballs, publishes to crates.io, and updates the Homebrew tap.

To cut a release:
1. Bump `version` in `Cargo.toml` and commit: `Bump to vX.Y.Z`
2. Push the commit to `main`
3. Create the release as a **draft**, with the notes it should ship with:
   ```sh
   gh release create vX.Y.Z --target main --draft --notes "..."
   ```
4. Create the tag, which is what actually triggers the workflow:
   ```sh
   gh api repos/thrashr888/cider/git/refs \
     -f ref=refs/tags/vX.Y.Z -f sha="$(git rev-parse origin/main)"
   ```
5. The workflow builds macOS binaries, uploads them to the still-draft release, publishes it, pushes to crates.io, and updates the Homebrew tap. Watch it with `gh run watch $(gh run list --workflow Release --limit 1 --json databaseId --jq '.[0].databaseId') --exit-status`.
6. Verify what shipped: `gh release view vX.Y.Z` should list both tarballs, and the tap formula should name the new version.

Both of the odd steps above are load-bearing, and each has already cost a release:

**The release must be created as a draft** (step 3). This repo has GitHub's immutable releases enabled: publishing freezes the release, and assets can no longer be uploaded — nor can it be reverted to a draft to fix. Creating it published is how v0.2.0 shipped with no binaries and never reached Homebrew. The workflow keeps it a draft while the tarballs upload and publishes it as its final step, so the notes written in step 3 are what ships.

**A draft does not create its tag** (step 4). GitHub holds the tag *name* on a draft but creates the ref only on publish — so a draft alone leaves the workflow waiting for a tag push that never comes. Do not reach for `git push origin vX.Y.Z` to solve it: repository rulesets block pushing tags. The git refs API is permitted and is what step 4 uses.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:7510c1e2 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->
