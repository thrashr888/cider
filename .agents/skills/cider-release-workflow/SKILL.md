---
name: cider-release-workflow
description: Maintain cider's release flow. Use when bumping versions, updating Cargo metadata, changing GitHub Actions release automation, or touching the Homebrew/crates.io publishing path.
---

# Cider Release Workflow

Read these files first:

- `Cargo.toml`
- `README.md`
- `.github/workflows/ci.yaml`
- `.github/workflows/release.yaml`
- Recent `git log --oneline`

## Release Model

- The crate version lives in `Cargo.toml`.
- Release tags are `v*`, and the workflow triggers when one appears.
- **The GitHub release is created as a draft, and the tag is created separately via the git refs API.** Both are load-bearing and each has already cost a release — see the Release Process section of `AGENTS.md` for the exact commands, and do not simplify the sequence back to a published release or a `git push` of the tag.
- CI runs on `macos-latest` and gates on `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test`, and `cargo build --release`.
- The release workflow builds `aarch64-apple-darwin` and `x86_64-apple-darwin` tarballs, uploads them to the waiting draft release, publishes it, publishes to crates.io, and updates the external `homebrew-cider` tap.

## Change Rules

- Keep artifact names and target triples aligned with `.github/workflows/release.yaml`.
- If the install story changes, update `README.md` examples at the same time.
- Preserve the Homebrew formula expectations: version, URLs, SHA fields, and `cider --help` smoke test.
- Match existing commit style for release prep. History uses concise subjects such as `Bump to v0.1.3`.

## Validation

- Run the local checks that mirror CI before touching release files.
- If you edit workflow logic, review every downstream dependency in the file: artifact names, tag names, formula generation, and push target.
- Leave the `draft: true` upload and the publish step that follows it alone unless GitHub's immutable-releases behavior changes. Publishing before the assets are attached cannot be undone: v0.2.0 is frozen without binaries because of it.
- Call out any step that cannot be fully validated locally, especially GitHub release, crates.io publish, and Homebrew tap updates.
