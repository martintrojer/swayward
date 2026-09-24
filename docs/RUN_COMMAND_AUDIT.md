This audit compares swayward's command parser with the commands that sway 1.12
makes reachable over IPC. It does not audit the other IPC request rows in
[`SWAY_COMPATIBILITY.md`](SWAY_COMPATIBILITY.md).

## Scope and method

This audit uses sway 1.12 commit
`88869399f421d9180dd8b6ed0b5a1f4a3585d252`, recorded in
[`tests/sway/compatibility.toml`](https://github.com/martintrojer/swayward/blob/main/tests/sway/compatibility.toml)
and matching `sway-ipc/fixtures/schema-version.json` in the pinned oracle. Sway selects
`command_handlers[]` while the configuration is active, then falls back to the
shared `handlers[]` table (`sway/sway/commands.c:44-129,162-173`). The tables
contain 82 unique command names because `fullscreen` appears in both.
`config_handlers[]` is excluded because sway consults it only while reading the
config file.

Each command in the TOML data has a complete invocation rather than a bare
command name. Most valid commands reject a bare name because an argument is
missing. The `parser` field records the result from
`swayward_ipc::command::parse`. The `execution` field records a source-level
comparison of every accepted command family with sway 1.12:

- `complete` means that swayward implements the audited syntax, target types,
  and state changes.
- `partial` means that a sway-supported form or target is missing, or that an
  accepted form has a different effect.
- A refused command returns a structured failure. Refusal is safer than an
  accepted command that silently ignores unsupported syntax or state.

The parser accepts 68 of sway's 82 unique runtime command names and rejects 14.
Of the accepted names, 24 implement the full audited command family and 44 are
partial. An accepted probe establishes one implemented form, not complete
command-family parity.

`urgent` counts as accepted because `urgent enable|disable|toggle` parses and
executes. Its separate `allow|deny` policy syntax remains fail-loud. The parser
also accepts `default_orientation`, `workspace_layout`, and `xwayland`, but
those names come from sway's config-only table and do not increase the runtime
count.

## Query the census

Run the validation and summary:

```sh
./contrib/command-census --check
./contrib/command-census
```

Use the JSON output to inspect current figures without copying the TOML into a
document:

```sh
./contrib/command-census --json | jq
./contrib/command-census --json | jq '{accepted: .commands_accepted, total: .commands_total, complete: .commands_complete, partial: .commands_partial}'
```

The script reads
[`tests/sway/compatibility.toml`](https://github.com/martintrojer/swayward/blob/main/tests/sway/compatibility.toml),
runs every recorded invocation through the parser, and checks the request and
event tables in [`SWAY_COMPATIBILITY.md`](SWAY_COMPATIBILITY.md). The TOML is
the per-command data source for sway source locations, probes, parser results,
execution status, state paths, classifications, and estimated cost. Markdown
is not generated from that data.

The prior 81-probe count omitted the duplicate-table normalization rather than
a sway command. This census uses unique runtime command names, which is the
stable quantity a user can invoke.

## Follow-up work

The audit groups accepted-command gaps into focused tasks:

- `runtime-accepted-criteria-targets`
- `runtime-criteria-language-parity`
- `runtime-parser-value-parity`
- `runtime-client-color-completeness`
- `runtime-default-border-semantics`
- `runtime-exec-startup-id`
- `runtime-pointer-policy-modes`
- `runtime-focus-layout-semantics`
- `runtime-floating-group-commands`
- `runtime-mode-definition`
- `runtime-swap-x11-id`
- `runtime-titlebar-command-semantics`
- `runtime-workspace-gap-forms`
- `runtime-mark-rule-reevaluation`

The existing `runtime-criteria-commands`, `runtime-set-command`,
`runtime-binding-commands`, `runtime-config-new-fields`, and
`runtime-subsystem-commands` tasks cover rejected command families found by the
same audit.
