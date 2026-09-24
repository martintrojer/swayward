# i3 suite against sway: calibration report

This report compares i3's unchanged test files against sway 1.12. It is a
calibration measurement, not a green gate and not a Wayland fork of the i3
suite.

The run used these revisions:

- i3 `9be3249ac5b377ed3270e36bca83df53d8023337`, the revision pinned by
  `tests/i3/README.md`
- sway `88869399`, reported as `sway version 1.12-88869399`
- swayward `c5fd5bc8` plus `eca93c61`

No file under `tests/i3/t/` changed. The runner reads the matching files from
the pinned i3 checkout. This avoids comparing sway against swayward's adapter.

## Method

`contrib/sway-calibration-run` builds i3's original `i3test.pm` from
`i3test.pm.in`. It replaces two harness files:

- `SocketActivation.pm` starts the pinned sway binary instead of i3. Sway runs
  with `WLR_BACKENDS=headless`, real Xwayland clients, one 1280×800 headless
  output, and a private `SWAYSOCK`.
- `swaycal::Shim` replaces i3's `I3_SYNC` barrier. Sway does not implement that
  X11 protocol. The replacement performs an X11 round trip and sway IPC round
  trips, then polls until two `GET_TREE` replies match.

The sync replacement is weaker than i3's `I3_SYNC` protocol. The Wayland event
loop does not guarantee ordering between the XWM and IPC file descriptors.
Window-lifecycle failures need an isolated rerun before they can support a
product conclusion.

Each test file runs in one `systemd-run --user --scope` with:

- `MemoryMax=2G`
- `MemorySwapMax=0`
- a 60-second timeout
- a private socket path
- a private `/tmp/.X11-unix` mount

The strict command was:

```sh
TIMEOUT=60 ./contrib/sway-calibration-run --all
./contrib/sway-calibration-report
```

A second command enabled the optional output-navigation shim:

```sh
SWAY_CAL_CONTENT_SHIM=1 TIMEOUT=60 \
  OUT="$PWD/target/sway-calibration-shim" \
  ./contrib/sway-calibration-run --all
./contrib/sway-calibration-report \
  --tapdir target/sway-calibration-shim
```

The report joins results only by the unchanged TAP assertion number. It does
not edit `tests/i3/coverage.toml`. `differential.tsv` contains the 901
attributable per-assertion rows from the optional pass.

## Strict result

The strict run covered all 242 vendored files as launch attempts. It produced
TAP in 73 files and reached 520 assertions. Of those assertions, 171 passed in
sway.

The other 169 files produced no TAP. The main blocker was i3's own workspace
navigation helper, not swayward's adapter:

```perl
my $content = first { $_->{type} eq 'con' } @{$output->{nodes}};
@cons = (@cons, @{$content->{nodes}});
```

Sway serializes workspaces directly below each output
(`sway/sway/ipc-json.c:869-874`). It does not serialize i3's intermediate
`content` container. The helper dereferenced `undef` in 140 files. In 136 of
those files, the abort happened before the first assertion.

Other no-TAP causes included `fake-outputs` assumptions, missing optional Perl
modules used by X11-specific files, and tests that own compositor process
lifecycle.

This strict result is the primary measurement. It shows that i3's original
harness cannot drive most of the suite against sway. Pointing the existing
swayward adapter at sway would not fix that problem. It would replace it with a
different measurement.

## Navigation-shim result

The optional pass changed only the five `i3test.pm` helpers that navigate from
an output through i3's `content` node. Those helpers instead selected direct
workspace children. The pass did not synthesize a node, rewrite `GET_TREE`, or
change a `.t` file.

This pass produced TAP in 208 files and reached 2,119 assertions. Sway passed
1,286 assertions. The remaining 34 files produced no TAP.

The shim weakens the oracle. A test that means to assert the existence of i3's
`content` node cannot do so through a navigation helper after this change.
Treat this pass as a layout sample that is conditional on direct workspace
navigation, not as raw i3-suite conformance.

## Differential

Against the optional pass, `contrib/sway-calibration-report` found:

| Row | Assertions or files | Interpretation |
| --- | ---: | --- |
| `SWAY_PASSES_DOCUMENTED_NONPASS` | 192 assertions | Sway passed an assertion that `coverage.toml` itemises as a swayward skip. These are candidates for premise review, not 192 swayward bugs. |
| `SWAY_FAILS_SWAYWARD_GREEN` | 350 assertions | Sway failed an assertion in a file whose non-skip assertions pass for swayward. These need source review because sway can intentionally differ from i3 and the swayward adapter can change the premise. |
| `BOTH_DIVERGE` | 359 assertions | Sway also failed an assertion that swayward itemises as a skip. This supports the classification, subject to the runner limits. |
| `COMPATIBLE` | 1,076 assertions | Both sides had compatible outcomes. |
| `UNATTRIBUTABLE` | 59 assertions | The file has unnumbered swayward failures, so `coverage.toml` cannot support a per-assertion join. |
| `NO_SWAY_DATA` | 34 files | Sway produced no TAP. No differential claim is possible. |

The strongest result is not a product defect. Of the 192
`SWAY_PASSES_DOCUMENTED_NONPASS` candidates, 135 skip reasons name an X11,
Xwayland, Wayland, or xdg-shell premise, and 31 name the bar surface. Real sway
received real X11 inputs and owns swaybar configuration. Swayward's current
adapter creates native Wayland clients and has no bar. Sway passing those
assertions confirms that the coverage row describes a harness boundary. It
does not show that swayward mishandles the same input.

Examples:

- `228-border-widths.t` assertions 7, 14, and 21 pass in sway. The assertions
  set `_NET_WM_WINDOW_TYPE_UTILITY` before map. The swayward skip already says
  that this Xwayland-only input has no native xdg-shell equivalent.
- `113-urgent.t` has 20 such candidates. Real X11 clients can mutate
  `WM_HINTS`; the native Wayland fixture cannot.
- `177-bar-config.t` has 23 candidates. Sway owns a bar configuration surface;
  swayward deliberately does not.

These rows are useful because they distinguish two claims that the current
`skip` label carries together: “sway differs from i3” and “the Wayland adapter
cannot establish the premise.” The measurement supports splitting those claims
in a later, individually reviewed coverage change. This task makes no such
change.

## Why sway fails 350 assertions that swayward passes

The `SWAY_FAILS_SWAYWARD_GREEN` row is not a compatibility advantage. A rerun
of all 61 files in that row, with the same pins and the same shim, reproduced
354 rows against the recorded 350; the difference is six IPC-event assertions
that flip between runs. Grouping the 354 by cause gives:

| Assertions | Share | Cause |
| ---: | ---: | --- |
| 127 | 35.9% | Sway has no `open` command. |
| 120 | 33.9% | The runner does not translate `fake-outputs`. |
| 36 | 10.2% | Sway spawned `swaynag`, which took 35px of usable height. |
| 16 | 4.5% | Sway refuses the command on an empty workspace. |
| 12 | 3.4% | The `layout stacked` alias is i3-only. |
| 12 | 3.4% | The shim's tick nonce is visible to the test. |
| 3 | 0.8% | Sway's default border differs from the one the file configures. |
| 28 | 7.9% | Not yet attributed to a cause. |

292 of the 354, or 82.5%, are harness artefacts: the first, second, third and
sixth rows. They measure the runner, not sway. Only rows four, five and seven
record a real i3/sway divergence, and together they are 31 assertions.

### `open` is i3-only (127 assertions)

i3's `open` creates an empty container, and `open_empty_con` wraps it
(`i3/testcases/lib/i3test.pm.in:382-387`). Sway's command tables do not contain
it (`sway/sway/commands.c:41-143`). Asked over sway's own IPC, sway replies:

```
open reply: { success => 0, error => "Unknown/invalid command 'open'" }
```

Every file in this group calls `open` or `open_empty_con` before its first
failing assertion, so the container the assertion inspects was never created.
Swayward's adapter implements `open` as a test-side action
(`src/tests/i3_conformance.rs:893-896`), which is why the same assertion passes
there. That is an adapter affordance, not compositor behaviour, and it is not
evidence that swayward handles an input sway mishandles.

### `fake-outputs` is untranslated (120 assertions)

i3's `fake-outputs` directive creates the multi-output geometry these files
need. Sway rejects it, which the logs show 22 times:

```
[ERROR] [sway/config.c:814] Unknown/invalid command 'fake-outputs'
```

The assertions then compare against `HEADLESS-1`, the one real headless output,
where the file expects `fake-0` or `fake-1`. The `Limits` section already
records that the run does not translate this directive. These 120 assertions
are the size of that limit inside this row.

### `swaynag` stole 35px of usable height (36 assertions)

This one is the runner's own fault and was not previously known. i3test writes
an `ipc-socket` line into every config (`i3test.pm.in:946`). Sway has no such
command, so `read_config` logs the error and then calls `swaynag_log`
(`sway/sway/config.c:811-819`), which spawns `swaynag` as a layer surface.
Every one of the 61 runs shows the result:

```
[sway/desktop/layer_shell.c:412] new layer surface: namespace swaynag
[sway/tree/arrange.c:274] Usable area for ws: 1280x765@0,35
```

765 instead of 800. The rounding is then off by exactly the amount the failures
report: a 25% share of 765px is 191px, and 191/765 = 0.24967, which misses
i3's `cmp_float` tolerance of 1e-6 by 3.3e-4. The reported delta is
9.80e-4, which is the same error under sway's width times height `percent`
product (`sway/sway/ipc-json.c:744-755`).

Adding `swaynag_command -` to the generated config
(`sway/sway/commands/swaynag_command.c:18-22`) removes the layer surface,
and restores `1280x800@0,0`. Measured directly on the six affected files, it
turns 15 failures into passes. Rerun across all 61 files of the row, it shrinks
the row from 354 to 335: 23 assertions leave and 4 new ones appear. The fix is
not applied here, because changing the generated config changes the premise for
every file in the suite. It belongs in a rerun of the whole measurement rather
than in a review of one row.

### Sway refuses the command on an empty workspace (16 assertions)

All 16 are in `554-commands-crash-for-window.t`, which runs each command on a
fresh empty workspace and asserts success. Sway declines, by design and with a
message in each case:

- `floating enable|disable|toggle` — "Can't float an empty workspace"
  (`sway/sway/commands/floating.c:27-28`)
- `move left|right|up|down|scratchpad` — "Can't move an empty workspace"
  (`sway/sway/commands/move.c:431-434`)
- `mark` and its `--add`, `--replace`, `--toggle` forms — "Only containers can
  have marks" (`sway/sway/commands/mark.c:21-22`)
- `layout default|stacked` — no container and no layout to resolve, so
  `get_layout` returns `L_NONE` (`sway/sway/commands/layout.c:95-114,168-170`)

This is a genuine i3/sway divergence and worth recording. Swayward's
coverage row for the file already itemises 18 skips, none of them these
assertions, so the two sides disagree about this file and the disagreement is
not yet explained. That is the strongest follow-up in this review.

### `layout stacked` is an i3-only alias (12 assertions)

i3 accepts both `stacked` and `stacking` (`i3/parser-specs/commands.spec:146-149`).
Sway's `parse_layout_string` accepts only `stacking`
(`sway/sway/commands/layout.c:11-21`). Sway's own IPC confirms both halves:

```
layout stacked  reply: { success => 0, error => "Expected 'layout default|tabbed|stacking|splitv|splith' or ..." }
layout stacking reply: { success => 1 }
```

The rejected command leaves the layout unchanged, so the next assertion reads
the previous layout and the file's later assertions inherit the drift. Swayward
parses `stacked` (`src/command/mod.rs:2550`), which is an i3 compatibility
choice, and `src/tests/i3_conformance.rs:283-286` already records the toggle
half of the same divergence.

### Not yet attributed (28 assertions)

These are candidate behaviour differences that this review did not bottom out.
Seven are in `274-move-branch-position.t` and `138-floating-attach.t` and
concern insertion position around tabbed and stacked branches. Four in
`141-resize.t` and one in `541-resize-set-tiling.t` are absolute pixel checks
that outlive the `swaynag` correction. Three are `001-tile.t:3`,
`137-floating-unmap.t:2` and `227-ipc-workspace-empty.t:1`, which are exactly
the X11 window-lifecycle shape the `Method` section says needs an isolated
rerun before it can support a conclusion; `227` also dies in
`xcb_intern_atom_reply`, which is an environment failure rather than a
behaviour one. The rest are single assertions across nine files.

None of these 28 has been shown to be a sway defect, and none supports a
swayward compatibility claim. They are the honest remainder.

### What this row does not show

It does not show that swayward is more i3-compatible than sway. 82.5% of the
row is the runner. Of the 31 assertions that are real divergence, swayward
passes them because it deliberately kept an i3 affordance sway dropped
(`stacked`) or because its adapter supplies a premise sway declines to supply
(`open`, empty-workspace commands). Both are design choices already recorded in
`docs/DIVERGENCE.md`. Neither is a defect in sway, and none of this belongs on
a public surface as a compatibility advantage.

## Floating-wrapper control

The task predicted a 35-assertion cluster involving i3's
`CT_FLOATING_CON` wrapper. Sway has no wrapper node. Most reached assertions in
this cluster failed or aborted at the expected raw-tree access. The control did
not cleanly produce 35 both-fail rows:

- `141-resize.t` assertion 83 passed accidentally because the helper compared
  two undefined wrapper-child rectangles. `tests/i3/README.md` already records
  this accidental pass.
- `218-regress-floating-split.t` assertion 1 likewise passed accidentally by
  counting children below an undefined wrapper child. The README also records
  it.
- `243-move-to-mark.t` aborted earlier at an X11 urgency input.
- Some `293-focus-follows-mouse.t` assertions depended on X11 absolute
  placement before they reached the wrapper lookup.

The observed cluster therefore supports the raw-tree wiring but does not prove
all 35 assertions independently. The exceptions match the limits documented
before this run.

## Limits

This measurement does not establish full sway compatibility for the vendored
suite.

- The strict pass reached only 520 assertions.
- The larger pass depends on a navigation shim that changes five i3test helper
  functions.
- Sway has no `I3_SYNC`; the settle replacement is weaker.
- Sway owns Xwayland. A sway restart destroys its X server, unlike an i3
  restart inside a persistent X server. Restart and separate-process rows need
  separate treatment.
- The first pass did not translate i3's `fake-outputs` directive. Multi-output
  setup failures therefore do not measure layout behavior.
- Config parser, registration, and process-lifecycle files remain mixed into
  the raw output. They are not layout evidence.
- Sway spawns `swaynag` in every run of this harness, because i3test's
  `ipc-socket` line is not a sway command and a config error triggers the
  nag. The layer surface takes 35px of usable height, so every absolute
  rectangle and every `percent` in the measurement is computed against
  `1280x765`, not `1280x800`. A future run should set `swaynag_command -`.
- The report can identify a documented skip by assertion number because skips
  are itemised. `coverage.toml` stores passing and failing assertions as
  counts, not numbered rows. `SWAY_FAILS_SWAYWARD_GREEN` is exact only for
  files with no swayward `fail` or `unreached` count. Other failures remain in
  `UNATTRIBUTABLE`.

## Decision

Keep this runner as an opt-in calibration tool. Do not add it as a green CI
gate. Do not update `tests/i3/coverage.toml` from these aggregate rows.

Review the 192 premise candidates by file before changing a label. Start with
X11-only inputs and bar configuration because the current reasons already name
the missing adapter premise. A future schema can distinguish a sway divergence
from an unproven adapter premise without changing the vendored test files.

## Reproduce the counts

`differential.tsv` includes only attributable non-compatible rows. Regenerate
all rows as JSON with:

```sh
./contrib/sway-calibration-report \
  --tapdir target/sway-calibration-shim
./contrib/sway-calibration-report \
  --tapdir target/sway-calibration-shim --json > report.json
./contrib/sway-calibration-report \
  --tapdir target/sway-calibration-shim --tsv > differential.tsv
./contrib/coverage-report --check
```

The TAP and sway logs live under `target/sway-calibration*` and are not
committed.
