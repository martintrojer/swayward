This page is the single reference for how swayward is tested. Other pages link
here instead of quoting test counts.

Every number on this page comes from a command, named next to it. Numbers go
stale: rerun the command for the current value. The oracle figures are those
recorded at the oracle revision pinned in
[`tests/oracle.toml`](https://github.com/martintrojer/swayward/blob/main/tests/oracle.toml)
(`db7147c`). They measure swayward commit `eb170906` against sway 1.12.

A passing test is evidence for the behaviour it exercises, not a compatibility
percentage.

## Where each check runs

| Check | Local | CI on every push ([`ci.yml`](https://github.com/martintrojer/swayward/blob/main/.github/workflows/ci.yml)) | Nightly ([`live-soak.yml`](https://github.com/martintrojer/swayward/blob/main/.github/workflows/live-soak.yml)) |
|---|---|---|---|
| Rust unit and headless tests | `cargo test --all` | `test` job | |
| Slow and randomized tests | `RUN_SLOW_TESTS=1 PROPTEST_CASES=20000` | `randomized-tests` job, 200,000 cases, release build | |
| In-process i3 suite (green files) | `cargo test --all` | `test` job | |
| Live IPC soak | `contrib/live-ipc-soak` | | 1 and 2 headless outputs |
| Black-box oracle runs | `contrib/sway-ipc-run`, `contrib/i3-suite-run` in the oracle | | |
| Clippy, rustfmt, MSRV, feature combinations | `./contrib/fast-gate` | `clippy`, `rustfmt`, `msrv`, `build` jobs | |
| Repository checks | `./contrib/fast-gate` | `internal-links`, `wiki-links`, `sway-to-kdl`, `release-consistency` jobs | |
| Visual tests | `cargo run -p swayward-visual-tests` | `visual-tests` job (build only) | |

`./contrib/fast-gate` runs, in order: nightly `cargo fmt --check`,
`contrib/check-rustfmt-fragments`, `cargo clippy --all --all-targets -- -D warnings`,
`cargo test --all`, `contrib/check-divergence`, and
`contrib/coverage-report --check`. When the change touches `src/layout/`, it
also runs both slow layout gates at 20,000 cases.

Black-box oracle runs are not part of CI. They start real sway and swayward
binaries and take hours across the full corpus.

## Inherited niri unit tests

Swayward forked niri at commit `9e72e491`
([`docs/FORK-BASE.md`](https://github.com/martintrojer/swayward/blob/main/docs/FORK-BASE.md)).
The niri test suite came with it.

| Measure | Count |
|---|---:|
| niri tests at the fork base | 217 |
| still present in swayward, by test function name | 180 |
| removed | 37 |

Command: `contrib/test-census --inherited <dir>`. It checks out the fork base
in `<dir>`, lists its tests with `cargo test -- --list`, and prints the removed
names.

Almost all of the 37 removed tests checked niri's scrolling layout: column
widths, view offsets, and column moves between workspaces. Swayward replaced
that layout with i3's nested container tree, so those behaviours no longer
exist. The surviving tests cover rendering, animation, input, configuration,
protocols, and the parts of window management swayward kept.

## Slow and randomized layout tests

These suites run zero or few cases unless `RUN_SLOW_TESTS` is set.

| Test | Without `RUN_SLOW_TESTS` | With it |
|---|---|---|
| `layout::tests::random_operations_dont_panic` | 0 cases | `PROPTEST_CASES` cases |
| `layout::tiling_tree` property tests | 16 cases | `PROPTEST_CASES` cases |
| `tests::ipc::property::random_commands_preserve_compositor_and_ipc_invariants` | 0 cases | `PROPTEST_CASES` cases |
| `tests::client_protocol_fuzz` | 16 cases | `PROPTEST_CASES` cases |

Run them locally with:

```sh
RUN_SLOW_TESTS=1 PROPTEST_CASES=20000 cargo test -p swayward --lib tiling_tree
RUN_SLOW_TESTS=1 PROPTEST_CASES=20000 cargo test --release -p swayward --lib random_operations_dont_panic
```

CI's `randomized-tests` job runs every test with `RUN_SLOW_TESTS=1` and
`PROPTEST_CASES=200000` in release mode.

`random_operations_dont_panic` comes from niri. Swayward rewrote its operation
set for the tree layout, so it now exercises splits, tabs, stacks, floating
groups, and workspace moves. It also checks the tree's invariants after every
operation.

### Regression seeds

When a randomized test fails, proptest records the failing case as a `cc` line
under `proptest-regressions/`. Every later run replays those seeds first.

| Seed file | Seeds |
|---|---:|
| `proptest-regressions/layout/tests.txt` | 50 |
| `proptest-regressions/tests/ipc/property.txt` | 16 |
| `proptest-regressions/layout/tiling_tree/tests/properties.txt` | 3 |
| `proptest-regressions/ui/mru/tests.txt` | 1 |
| `proptest-regressions/tests/client_protocol_fuzz.txt` | 0 |

Command: `contrib/test-census`.

A fixed failure gets both its seed line and a named regression test that holds
the shrunk operation sequence. The seed reproduces the generated case. The
named test records the minimal behaviour and stays readable when the generator
changes.

## Swayward unit and headless tests

`cargo test --all` lists 1,219 tests:

| Crate | Tests |
|---|---:|
| `swayward` | 1,034 |
| `swayward-ipc` | 103 |
| `swayward-config` | 82 |
| `swayward-visual-tests` | 0 |

The `swayward` crate by area:

| Area | Tests | What it covers |
|---|---:|---|
| `tests::ipc` | 453 | Sway IPC requests, replies, events, and commands over a real socket |
| `layout` | 329 | The container tree, workspaces, floating, and fullscreen |
| `tests::i3_conformance` | 29 | The in-process i3 suite runner and its validation |
| `tests::floating`, `tests::floating_sway` | 34 | Floating placement and sizing |
| `utils` | 27 | Shared helpers |
| `tests::layer_shell` | 18 | Layer-shell clients |
| other `tests::` modules | 89 | Headless protocol and window-management tests, including xdg-shell, border resize, session lock, screencopy, output management, and client protocol fuzzing |
| other modules | 55 | Unit tests for input, IPC helpers, UI, commands, protocols, animation, backend, D-Bus, and CLI |

Command: `contrib/test-census` (run it in the dev container). It prints every
area separately.

The tests under `src/tests/` start a real compositor in the test process and
connect real `wayland-client` clients to it. No nested session or display is
needed, so CI runs them. IPC tests talk to a private socket created for each
test. They never touch the session's `SWAYSOCK`.

Many IPC tests compare whole replies with values captured from real sway 1.12.
[IPC oracle coverage](https://github.com/martintrojer/swayward/blob/main/docs/IPC_ORACLE_COVERAGE.md)
records which mutations these tests catch, and which parts of each reply they do
not check.

## Fuzzing and property tests

| Test | Input | What must hold |
|---|---|---|
| `random_operations_dont_panic` | Random layout operations | No panic, and the tree invariants hold after each operation |
| `random_commands_preserve_compositor_and_ipc_invariants` | Random sway commands on two outputs | The tree invariants hold. `GET_TREE` and `GET_WORKSPACES` parse, every mapped window appears once, and every focus ID is live. |
| `client_protocol_fuzz` | Random xdg-toplevel requests from a real client | Each request gets its expected outcome |
| `contrib/live-ipc-soak` | Command fuzz, wire fuzz, and random commands with output hotplug, against a release build | Every request gets a reply, and the compositor log has no `ERROR` or panic |

The nightly soak runs 10,000 random commands after 200 seeds each of command
fuzz and wire fuzz, once with one headless output and once with two. A failed
job uploads the probe and compositor logs.

The oracle's own command-fuzz and wire-fuzz corpora are described in
[Sway IPC snapshots](#sway-ipc-snapshots).

## The oracle: i3's own test suite

The [sway IPC oracle](https://github.com/martintrojer/sway-ipc-oracle) is a
separate repository. It holds i3's upstream Perl tests unchanged, plus the
adapters that run them against i3, sway, and swayward. A test you can edit to
pass is not evidence, so swayward never edits these files.

The adapter replaces X11 window creation with real Wayland clients. It does not
rewrite assertions.

### Black-box run

The oracle runs each compositor as a separate process and records every
assertion.

| Measure | sway 1.12 | swayward |
|---|---:|---:|
| Passing assertions | 1,558 | 1,478 |
| Non-passing assertions | 2,197 | 2,277 |
| Non-passes shared by both | 1,936 | 1,936 |
| Non-passes for one compositor only | 261 | 341 |

Command: `contrib/i3-suite-summary .cache/sway-ipc-oracle`.

Sway fails most of i3's suite too, because many assertions test i3 itself: its
X11 window manager, config parser, bar, in-place restart, or tree nodes that
sway does not expose. Comparing with sway is therefore more useful than the raw
count. After removing the reviewed X11-only assertions in the five files with
the largest gaps, swayward passes 1,217 of the 1,447 assertions that sway passes
(84%). [Reading the i3 suite results](https://github.com/martintrojer/swayward/blob/main/docs/I3_SUITE_RESULTS.md)
breaks down the remaining rows.

### In-process run

Swayward also runs the same unchanged files inside its own test process. This
is faster and runs on every build.
[`tests/i3/coverage.toml`](https://github.com/martintrojer/swayward/blob/main/tests/i3/coverage.toml)
is the source of truth: one row per file, with every non-passing assertion
itemized.

| Measure | Count |
|---|---:|
| Files | 242 |
| Fully green files | 99 |
| Declared assertions | 3,171 |
| Pass | 2,208 |
| Documented skip | 877 |
| Fail | 30 |
| Unreached | 56 |

Command: `contrib/coverage-report`.

A documented skip cites the sway source that makes the assertion inapplicable,
or the exact harness premise it cannot meet. Failures and unreached assertions
are open work. `contrib/coverage-report --check` fails on an undocumented skip.

`cargo test --all` runs the fully green files. To check that no other file lost
a pass, run the ratchet:

```sh
SWAYWARD_I3_RATCHET=1 cargo test -p swayward --lib i3_conformance_non_green -- --nocapture
```

Count a single file's assertions with `contrib/tap-count`, never by hand. Only
the first unindented TAP plan counts.

## Sway IPC snapshots

The oracle also compares swayward's IPC replies and events with captures from
the pinned sway build. Each corpus starts a fresh compositor per scenario,
normalizes values that legitimately vary between runs, and compares the rest
exactly.

| Corpus | Rows | sway 1.12 match | swayward match | swayward mismatch |
|---|---:|---:|---:|---:|
| Hand-picked state scenarios | 550 | 550 | 499 | 51 |
| i3-derived states | 4,010 | 4,010 | 3,568 | 442 |
| Random sequences (500 seeds, 20 steps) | 500 | 500 | 256 | 244 |
| Events | 45 | 45 | 32 | 13 |
| Command fuzz | 24 | 24 | 24 | 0 |
| Wire fuzz | 10 | 10 | 5 | 5 |

Commands: `contrib/i3-suite-summary .cache/sway-ipc-oracle` for the swayward
columns. The sway columns are the `[run]` tables of
`sway-ipc/results/sway-1.12-*.toml` in the oracle. Sway matches its own
captures on every row, which shows the normalization does not hide real
differences between two runs of one compositor.

Normalization rules live in the oracle's `sway-ipc/normalize.toml`. Every
ignored field there has a written reason. Examples are node IDs, process IDs,
backend output names, and rectangle sizes that depend on font metrics.

To compare a local build against one scenario or seed, run the oracle runner
from an oracle checkout:

```sh
./contrib/sway-ipc-run --compositor swayward --binary <path> --scenario <name>
./contrib/sway-ipc-run random --compositor swayward --binary <path> --seeds 3,17
```

`contrib/targeted-oracle` in the swayward repository selects the scenarios and
seeds that a change can affect.

## Intentional differences from sway

[Known deviations](https://github.com/martintrojer/swayward/blob/main/docs/KNOWN_DEVIATIONS.md)
lists every user-visible difference from sway, with the sway source it departs
from. The tests encode these in three ways:

- **A documented skip** in `tests/i3/coverage.toml`, when an i3 assertion
  checks behaviour that sway and swayward deliberately do not share.
- **A swayward test that pins the difference.** For example, malformed IPC
  frames get a failure or a closed socket where sway waits forever. The wire
  fuzz tests assert swayward's reply.
- **A recorded oracle mismatch.** Four of the five wire-fuzz mismatches are that
  same deliberate safety change, and Known deviations names each case.

On the wire, a request is answered exactly as sway answers it, or it returns
`{"success": false}`. Swayward never returns a sway-shaped reply with different
content. `GET_CONFIG` is the example: sway returns its config file, swayward's
config is KDL, so the request is refused rather than approximated.

## Other checks

- **Visual tests.** `swayward-visual-tests` is a GTK app that renders
  hard-coded scenes with the real layout and rendering code and mock windows.
  A person inspects them. CI only builds it.
- **Decoration captures.** `contrib/capture-decoration-matrix` renders
  titlebars and borders in a headless session, with square and rounded corners
  side by side, for review by eye.
- **Window-opening snapshots.** `tests::window_opening` compares the configure
  sequence for each window-rule combination against 1,560 `insta` snapshots in
  `src/tests/snapshots/`.
- **No panics in the IPC library.** `swayward-ipc` parses untrusted client
  input. Its library code denies `unwrap`, `expect`, unchecked indexing and
  slicing, and explicit panics through crate-level Clippy lints.
- **Repository checks.** `contrib/check-divergence` requires a ledger entry for
  every edited niri file. `contrib/check-internal-links` keeps public docs from
  linking to internal notes. `contrib/sync-github-wiki --check` resolves wiki
  links. `contrib/command-census --check` validates the sway command census
  and the figures quoted from it. `contrib/coverage-report --check` validates
  the i3 coverage ledger.
- **Formatting.** rustfmt runs on the nightly toolchain, because
  `rustfmt.toml` uses nightly-only options.
