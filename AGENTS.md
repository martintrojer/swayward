# swayward — agent conventions

swayward is a fork of niri that replaces the layout engine with i3's nested
container tree and the IPC layer with sway's protocol. Design of record:
[`docs/specs/2026-09-12-swayward-foundation.md`](docs/specs/2026-09-12-swayward-foundation.md).

## Build and test

The host is immutable (Fedora Atomic). Everything builds in a container:

```
distrobox enter swayward-dev -- bash -lc 'cd <repo> && cargo test --all'
```

Setup and baseline numbers: [`docs/BUILDING.md`](docs/BUILDING.md).

Format with **nightly**: `cargo +nightly fmt --all`. `rustfmt.toml` uses four
nightly-only options, and the CI fmt job runs on nightly. Stable rustfmt reports
clean while leaving nightly diffs in place — that trap has already cost one
round-trip.

## Running a nested compositor: cap it or it eats the machine

Testing against a live swayward means running a second compositor and real
clients inside the developer's own session. That has already OOM-killed a
machine: a test waybar reached **9 GB RSS / 34.6 GB virtual** before the kernel
stepped in, dragging swap down with it.

A runaway client must fail fast and locally instead of exhausting the host.
**Cap memory and wall time, and reap on exit:**

```bash
#!/bin/bash
trap 'pkill -9 -f "^\./target/debug/swayward"; pkill -9 -f "$MY_CFG"' EXIT

systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 \
  timeout 60 ./target/debug/swayward -c /tmp/swcfg/config.kdl
```

`MemorySwapMax=0` is the load-bearing half. Without it a leak grinds the machine
through 31 GB of swap for minutes before anything dies.

Four rules that follow from that incident:

- **One script, one lifetime.** Launch, query and reap inside a *single* shell
  invocation. Agent tool calls are isolated processes, so a `pkill` in a later
  call cannot see what an earlier call started — that is how a dozen orphaned
  compositors accumulated unnoticed.
- **Strip `spawn-at-startup` from the test config.** A spawned bar competes for
  the IPC socket and can starve `swaymsg`:
  `grep -v 'spawn-at-startup' ~/.config/swayward/config.kdl > /tmp/swcfg/config.kdl`
- **Match the exact binary when reaping.** `pkill -f swayward` also matches your
  own shell command line and the operator's session. `pgrep -af
  '^\./target/debug/swayward'` does not.
- **Never kill the operator's processes.** Their `sway` and their `waybar` run
  alongside yours. Identify yours by the pid you launched, not by name.

Prefer the headless harness wherever it can answer the question. `src/tests/`
drives a real compositor and real `wayland-client` clients with no nested session
at all — `tests::floating::two_windows_tile_side_by_side_and_focus_follows`
replaced a manual two-terminal check that way, and unlike a human watching a
screen it runs in CI.

## History is rebase fuel

We merge upstream niri releases forever (`git fetch upstream --tags && git merge
vXX.YY`). Every commit we author is a commit a future merge has to reason
through, so **our history is a tool for rebasing, not a diary of how the work
happened.**

Rewrite freely while a branch is unmerged. `origin` is ours alone and nothing
depends on our shas:

- **Amend** rather than adding "fix typo", "address review", "actually fix it".
- **Squash** a task's exploratory commits into the one commit that lands.
- **Force push** after rewriting. Expected, not exceptional.

One commit should be one reviewable idea. If a future merge conflicts, the
person resolving it should be reading a single coherent change, not
reconstructing intent from six increments.

Corollary: keep unrelated changes out of a commit. A rename commit contains only
renames; a fmt commit contains only formatting. Mixed commits are the ones that
turn a trivial merge into an archaeology problem.

## Divergence ledger

Prefer new modules over editing inherited files. When an inherited file genuinely
must change, add a line to [`docs/DIVERGENCE.md`](docs/DIVERGENCE.md) saying what
and why. Making the cost visible beats forbidding the edit and being ignored.

## Invariants

Full statements with rationale live in the spec. In short:

| | |
|---|---|
| **I1** | Every `SWAYSOCK` reply is byte-schema-identical to sway's, or a well-formed `{"success":false,"error":"…"}`. Never a third thing, never a hang. Golden fixtures in `tests/fixtures/sway/` are the oracle — captured from real sway, never hand-edited to make a test pass. |
| **I2** | The tree is always well-formed. `check_invariants()` runs after **every** proptest op, not per sequence. |
| **I3** | Never panic in a path reachable from a live session. |
| **I4** | An `ERROR` in the log is a bug. Use `warn!` for user and hardware misbehaviour. |
| **I5** | Upstream diff is a budget; see the divergence ledger. |
| **I6** | No feature without the thing that proves it works. Retired is fine; silently broken is not. |
| **I7** | Solid beats fast. Stability regressions outrank new capability. |

## Tests

Extend the inherited harness; do not build new infrastructure.

- Headless compositor plus real `wayland-client` test clients: `src/tests/fixture.rs`.
- New layout mutations go in the proptest `Op` enum so they are fuzzed automatically.
- Slow gate: `env RUN_SLOW_TESTS=1 PROPTEST_CASES=20000 cargo test -p swayward tiling_tree`.
- Visual features get a case in `swayward-visual-tests`.

`#[ignore]` on a failing inherited test is a silent lie about coverage. Port it
to tree semantics, or delete it with a one-line reason.
