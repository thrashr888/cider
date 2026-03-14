---
name: cider-output-contract
description: Preserve cider's CLI output contract. Use when changing JSON fields, pretty rendering, schema output, dry-run behavior, envelopes, pagination, or write-result shapes.
---

# Cider Output Contract

Read these files first:

- `src/main.rs`
- `src/pretty.rs`
- `src/sources/util.rs`
- `README.md`

## Contract To Preserve

- Default stdout is compact JSON.
- `--pretty` is presentation-only and belongs in `src/pretty.rs`.
- `--envelope` wraps output as `{"ok": true, "data": ...}`.
- Mutating commands return `ActionResult` with `ok`, `action`, optional `id`, and optional `message`.
- `--dry-run` returns the `DryRunResult` shape from `src/main.rs` and should not perform side effects.
- `schema` must stay aligned with the commands that support dry-run, pagination, and stable IDs.

## Change Rules

- Treat output changes as compatibility-sensitive. Prefer additive fields over renames or shape changes.
- Keep errors and warnings on stderr, not stdout.
- If you change pretty output, verify that compact JSON is unchanged.
- If you add optional fields, use `skip_serializing_if` where omission is cleaner than `null`.
- If you add a new write command, make sure the action string is short and stable.

## Validation

- Add or update inline tests in `src/pretty.rs` or the affected source module.
- Manually check representative commands in both compact and pretty forms.
- Check `cargo run -- schema --pretty` after schema changes.
- Check a representative `--dry-run` path for any new mutating command.
