# i3 conformance tests

This directory contains unmodified test files from i3 commit
`9be3249ac5b377ed3270e36bca83df53d8023337`. They retain i3's BSD license in
[`LICENSE`](LICENSE).

The Rust test runner starts swayward's existing headless compositor and real IPC
server. The small `lib/i3test.pm` adapter replaces only i3's X11 process and
window setup: commands and tree queries use swayward's IPC socket, while
`open_window` asks the Rust runner to create a real Wayland client. Assertions
and expected values remain in the upstream `.t` files.

The runner requires Perl with `Test::More` and `JSON::PP`. On Fedora install
`perl-Test-Simple perl-JSON-PP`; on Debian or Ubuntu install
`perl libtest-simple-perl libjson-pp-perl`. `contrib/dev-container.sh` installs
the Fedora packages. The runner uses a 1280×800 output with zero gaps to match
i3's `testcases/lib/StartXServer.pm:106-108` and `testcases/i3-test.config`.
It disables window movement and resize animations and makes its Wayland clients
acknowledge and commit the latest configure around floating and resize commands.
Green conformance tests therefore cover settled geometry, not animated
intermediate states or clients that delay or omit configure acknowledgements.
Render-versus-layout geometry bugs are outside this oracle: native tests must
leave animations enabled and sample commands or IPC while motion is in progress.
`open_empty_con` also creates a real Wayland window because swayward cannot
create an empty container through IPC.

The adapter gives each `open_window` object the real swayward container id. Its
X11 `mapped` compatibility check reads workspace visibility from
`GET_WORKSPACES`; sway's `GET_TREE` `focused` flag identifies only the single
default-seat focus node, not every visible workspace. Native `xdg_shell` nodes
keep sway's schema and omit the X11 `window` field (`sway/sway/ipc-json.c:670-683`).
Tests that read that field are classified rather than receiving an alias to
`node.id`; the two identifiers do not prove the same interface.

### Floating wrappers are i3-only

i3 creates a distinct `CT_FLOATING_CON` parent when floating a container, puts
the selected container below it, and serializes the parent as `floating_con`
(`i3/src/floating.c:232-351,457-460`; `i3/src/ipc.c:361-383`). Sway has no
floating-wrapper node type. It stores the floating container itself in the
workspace floating list and serializes that container directly in
`floating_nodes` (`sway/sway/ipc-json.c:478-484,532-540`). An i3 lookup through
`floating_nodes[n].nodes[0]` is therefore one level too deep for a sway-shaped
tree.

This is a permanent oracle limit, not a missing adapter feature. Synthesizing
that parent only for the Perl client would make the test observe a tree that a
real sway IPC client never receives. Direct-node diagnostics and native tests
can prove the underlying behavior, but the unchanged assertion remains i3-only.
The difference affects 35 assertions in 11 files: `124-move.t` (4),
`135-floating-focus.t` (10), `141-resize.t` (2), `174-border-config.t` (2),
`218-regress-floating-split.t` (1), `228-border-widths.t` (3),
`236-floating-focus-raise.t` (6), `243-move-to-mark.t` (1),
`279-regress-default-floating-border.t` (1), `287-edge-borders.t` (1), and
`293-focus-follows-mouse.t` (4). All 35 are classified as i3-only skips. Assertion 83
in `141` compares two undefined rectangles, and assertion 1 in `218` counts
children below an undefined wrapper child as zero. Those two accidental passes
are not evidence for the asserted behavior. Files that also mention a wrapper but cannot reach
or isolate the assertion because of X11 input, pointer input, layout restoration,
or floating-split behavior are not included in this count.

For an `i3_config` import, the adapter sends the complete config to the Rust
runner. The runner translates it with the shipped `contrib/sway-to-kdl` tool and
refuses to run if the translator reports any manual-attention item. The adapter
maps the i3 suite's X11 `class` and `instance` criteria to the Wayland `app_id`
criterion. This is a portability substitution, not sway criteria equivalence:
sway matches native Wayland views by `app_id` and evaluates `class` and
`instance` only for Xwayland views (`sway/sway/criteria.c:243-259,355-390`).
These tests therefore prove equivalent matching behavior against the one
identity the adapter can assign, but do not prove X11 class or instance
matching. `open_window` rejects a distinct `instance` and rejects `before_map`
instead of ignoring values that a native `xdg_toplevel` cannot carry. The
protocol exposes one application identity through
`xdg_toplevel.set_app_id` and no X11 instance or window-role property
(`xdg-shell.xml`, `xdg_toplevel.set_app_id`). The translator supports `assign`
and the `floating enable` or `floating disable` subset of `for_window`. It does
not silently discard unsupported directives.

The adapter removes only the exact test-only `bar { # Disable i3bar. i3bar_command : }` block used by i3 tests that do not need a bar. Sway has no `i3bar_command`, and the headless fixture starts no bar, so retaining the block would change nothing except making translation fail. Arbitrary bar blocks remain fail-loud in the product translator. Five upstream files mention `i3bar_command`; only `504-move-workspace-to-output.t` is otherwise protocol-portable. `201-config-parser.t` tests bar parser errors, `264-dock-criteria.t` and `526-reconfigure-dock.t` require X11 docks, and `555-i3bar-workspace-output-assignment.t` launches a real i3bar command.

The runner handles i3's `fake-outputs` test directive separately because it
configures i3's test server rather than normal sway configuration. Each
`WIDTHxHEIGHT+X+Y` entry creates a real headless output named `fake-N` through
the compositor's output-add path. Sizes, positions, count, and optional `P`
markers are parsed; the zero-origin output remains primary by the fixture's
normal insertion order. Of i3's 285 tests, 57 use `fake-outputs`; 38 are among
the 217 tests without the initial X11 protocol exclusions.

The adapter also preserves `open_window(dont_map => 1)`: it creates the
`xdg_toplevel` without committing the surface, and the test's later `map` call
performs the initial commit, configure acknowledgment, and buffer attachment.

Sway defaults `focus_follows_mouse` to `yes`, while swayward's native default is
off. The translator emits `focus-follows-mouse` when a sway config omits the
directive, removes it for `focus_follows_mouse no`, and refuses `always` because
swayward cannot reproduce its behavior after workspace switches
(`sway/sway/config.c:272`; `sway/sway/input/seatop_default.c:591-595`). The
test runner's enabled fallback therefore does not override an explicit `no`.

For `240-focus-on-window-activation.t` only, the adapter recognizes the test's
`_NET_ACTIVE_WINDOW` message shape and substitutes a real xdg-activation token
request plus activation request for the same native Wayland surface. Other X11
events still die loudly. This includes EWMH client messages consumed by an X11
window manager: `xwayland-satellite` presents the result as an ordinary
`xdg_toplevel`, and xdg-shell has no request that carries an absolute target
rectangle. Testing those messages requires a real X11 client and the satellite;
the adapter does not fabricate either side of that protocol boundary. The token deliberately has no input serial, matching
clients that request attention without a user gesture; swayward marks such
tokens urgency-only by default (`src/handlers/mod.rs:787-852`). The translated
`urgent`, `focus`, and `none` modes override that default with `set-urgent`,
`focus`, and `ignore` window rules. Sway's `smart` mode is not translated because
it branches on workspace visibility and has no equivalent window-rule value
(`sway/tree/view.c:477-512`).

The adapter's `warp_pointer` drives swayward's real pointer and rendered-geometry
hit-test path. It settles pending client configures and animations first, then
applies sway's default focus-follows-mouse behavior (`sway/config.c:272`). Of the 17 upstream files that
call `warp_pointer`, 12 are in the 217-file portable set. The other five require
X11 client rectangles, XTEST, shape, or pointer-query protocol.

The adapter derives `focused_output` exactly as upstream i3test does: it reads
`GET_TREE`, takes the first id in the root `focus` array, and finds that id among
the root output nodes (`i3/testcases/lib/i3test.pm.in:700-710`). It does not use
`GET_OUTPUTS.focused`; real sway tree captures leave output-node `focused` false
while root `focus[0]` identifies the seat-focused output.

Upstream has 49 test files that reference `X11::XCB`; all 49 are among the 68
files classified as X11-touching, so none belongs wholly to the 217 portable
set. Some contain useful portable prefixes. The local `X11::XCB` module exports
only numeric constants needed to compile such files. Constants and `Rect` are
inert values, not simulated X11 state. `X11::XCB::Window` is only a loadable
package for the adapter's existing Wayland-backed window object. Constructors
that imply a real X connection or X11 size-hint state die loudly; all other X11
operations remain absent and likewise fail at their call sites. The stubs do not
fabricate X11 window identity, properties, events, or protocol behavior.

Sway sorts its stored workspace list when it creates or moves a workspace
(`sway/sway/tree/workspace.c:255-259`; `sway/sway/tree/output.c:387-404`). The
IPC serializer and relative navigation preserve that stored order. Swayward now
does the same and no longer stores niri's trailing unnamed workspace. Each
output keeps at least one real workspace. Empty inactive workspaces survive only
when the configuration declares them persistent, and drag creation remains
preview state until drop.

The unchanged `117-workspace.t` passes its first 50 assertions, then aborts in
its i3-specific `workspace_numbers_sorted` helper. The helper requires an
output-level `content` node, which sway does not serialize. The later assertions
therefore remain unproven by that file; `tests/i3/coverage.toml` records the
current assertion-level classification.

The local `ExtUtils::PkgConfig` adapter implements only `atleast_version` by
executing the host's real `pkg-config --atleast-version`. This keeps upstream
library-version guards honest without requiring an untracked Perl module in the
development container. Every other `ExtUtils::PkgConfig` method dies loudly;
the adapter does not provide package flags or metadata.

The adapter cannot reproduce a compositor restart. Rebuilding `Fixture` destroys
its Wayland clients and windows, while `State::reload_config` preserves them and
is not a restart. `launch_with_config` is supported only for independent config
phases: it reloads the requested config, and `exit_gracefully` removes all test
windows before the next phase. Tests that depend on state surviving a restart
remain unproven. In particular, `176-workspace-baf.t` launches two compositor
configurations and later verifies back-and-forth state across `restart`; using
reload for either transition would test a weaker lifecycle.

The adapter cannot reproduce the position in i3's
`open_window(rect => [x, y, width, height])` input or later X11
`Window::rect(Rect)` configure requests. The i3 helper passes the initial
rectangle when it creates an X11 child window
(`i3/testcases/lib/i3test.pm.in:313-350`), and i3 applies later configure-request
position and size fields to floating leaves (`i3/src/handlers.c:300-371`). A
native `xdg_toplevel` client can choose its initial content size through the
buffer and `xdg_surface.set_window_geometry`; `194-regress-floating-size.t` uses
this size-only substitution. The protocol does not let the client choose an
absolute position. `xdg_surface.set_window_geometry` describes visible bounds in
surface-local coordinates and says changing its x/y should generally not alter
the window's position (`xdg-shell.xml`, `xdg_surface.set_window_geometry`). The
toplevel `move` and `resize` requests start interactive compositor-controlled
operations, require an input-event serial, and do not accept a target size or
position (`xdg-shell.xml`, `xdg_toplevel.move`, `xdg_toplevel.resize`). The
adapter's later rectangle setter therefore dies loudly; assertions that depend
on X11 configure requests remain unproven.

### Per-file harness branch audit

An audit at commit `working-tree` found 27 textual references to
`SWAYWARD_I3_TEST` in the current harness. The count fell from 69 when the
broad assertion-name, assertion-number, and subtest interceptors were deleted.
Those branches emitted TAP skips before comparing values; the remaining branches
select file-level inability-to-start handling, portable input substitutions, or
the single exact i3-error-wording assertion documented in the coverage ledger.
A test asserts the total against the harness, so a new branch remains a
deliberate, reviewable change.

| Category | References | Count | Audit result |
| --- | --- | ---: | --- |
| Portability substitution | `194` initial-floating path and four size fields; `531` fullscreen request; `295` activated state; `240` activation event; `287` client rectangle | 9 | Each branch drives a native Wayland operation that preserves the tested behavior. The corresponding coverage rows cite sway's xdg-shell, activation, focus, and geometry behavior. |
| Cited skip | `159`/`196`/`235`/`262`/`540` skip dispatch and reason selection; assertion skips for `193`, `260`, `294`, `287`, `228`, `518`, `194`, `541`, `319`, `133`, `166`, `551`, and `164` | 15 | Each branch marks an assertion or file that cannot hold against sway or the Wayland harness. Each file stays out of `passing.txt`; its coverage row gives the source citation and boundary. The five-file skip-all dispatch and its two-way reason selection count as two references. |
| Setup or lifecycle adaptation | `298` removes i3's unsupported `ipc_kill_timeout 500` test directive; `202` finds sway's nested scratchpad workspace; `289` preserves shutdown subscriptions and requests in-process exit; `553` maps transient parents and isolates i3's unsupported `all` policy | 4 | The row records sway's fixed 4 MB write-buffer behavior and the native backpressure test. |
| Weakens the oracle | `201`; both `238` checks; `509`; `307`; `510`; `231`; `245`; `320`; `513`; `120`; `169`; `512`; `550-split-redundant-containers`; `302`; and both `527` checks | 17 | Fifteen files containing 78 TAP skips were listed as passing in full. This masked the cited incompatibilities as false greens. The audit removed all fifteen from `passing.txt` and deleted the duplicate, unreachable `509` branch. |

The category-(d) branches did not conceal an unknown compositor regression: the
file rows already recorded each skip and its sway source. They weakened the
manifest oracle by letting a zero exit status with TAP skips count as a full
pass. The default manifest now contains only files with no TAP skips.

`tests/i3/passing.txt` lists the files that pass in full, and the default gate
runs every one of them. A conformance slice adds its file to that list only when
all assertions pass without skips. This keeps coverage from silently rotting
and lets slices land in parallel without editing the same Rust source.

The runner enforces the no-skips rule, because the manifest once did not. An
audit of the per-file branches in `tests/i3/lib/i3test.pm` found fifteen listed
files between them emitting seventy-eight cited TAP skips, which the manifest
reported as fully green. The coverage rows below had recorded those skips all
along, so no product regression was hidden -- the scoreboard was wrong, not the
compositor. Removing them took the green count from 108 to 93. A manifest entry
that emits any skip now fails the runner, so the count cannot drift that way
again.

Run the full passing set with `cargo test -p swayward i3_conformance_runner`.
Select another vendored file with, for example:

```sh
SWAYWARD_I3_TEST=122-split.t cargo test -p swayward i3_conformance_runner -- --nocapture
```

Passing files keep the adapter wired into the normal test gate. Other vendored
files intentionally retain their failing assertions: those failures are
conformance findings, not expectations to bless or silently skip. See the task
report for assertion-level results.

Count assertions from the first unindented TAP plan line (`^1\.\.[0-9]+$`) in
the test's output. An explicit plan appears at the top; `done_testing` writes it
at the bottom. Do not count `ok` or `not ok` lines from a Rust test failure: the
panic message repeats the complete child TAP stream. Indented plans belong to
subtests. When a row reports inner assertions instead of the top-level plan,
label that distinction in the Assertions column. If a test exits before its
final plan, report only the number reached in the original child output; do not
infer a total from repeated panic diagnostics.

`contrib/tap-count` applies those rules to a log, because following them by hand
has produced wrong counts in both directions. It also separates TAP skips, which
are written `ok N # skip reason` and would otherwise inflate the pass count: a
file can exit zero with skips and still not pass in full. It splits the output into streams
wherever the numbering restarts, marks the first as authoritative, and prints the
figures to quote:

```sh
SWAYWARD_I3_TEST=297-assign-workspace-to-output.t \
    cargo test -p swayward --lib i3_conformance -- --nocapture 2>&1 | contrib/tap-count
```

The manifest runner wraps every file with its filename, including setup,
configuration, control-socket, rejected-command allow-list, and timeout panics;
TAP failures additionally retain their assertion and source-line summary. A
residual full-suite failure observed after the duplicate-mark fix could not be
reproduced in 10 isolated manifest runs or 10 full-suite runs under the required
2 GiB memory cap. Isolated manifest runs took 36.27–36.64 seconds after warm-up;
full-suite runs took 42.15–43.08 seconds for the swayward test binary. The
original 62-second failure remains unexplained, so the timeout remains 30
seconds per vendored file rather than being raised without evidence.

## Coverage conventions

### Where the numbers come from

**`tests/i3/coverage.toml` is the source of truth.** Run
`contrib/coverage-report` for the current census or add `--json` for
machine-readable output. Edit the TOML, never census prose.

**The target state: every assertion either passes, or is a documented skip.**
Nothing else is an acceptable end state.

That target decides which fields carry detail. A skip is *permanent*: it claims
the test's expectation is not sway's behaviour, so it is itemised per assertion
and must cite the sway or i3 source that proves it. Without the citation a skip
is just a failure wearing a better label. A `fail` or `unreached` assertion is
*temporary* -- it is work to be done, not a finding to be recorded -- so it is a
plain count. No detail is owed for something meant to disappear.

```toml
[files."319-gaps.t"]
assertions = 28
pass = 25
fail = 1

[[files."319-gaps.t".skip]]
n = 19
reason = "i3 reapplies workspace gap assignments on reload; sway copies gaps when creating a workspace"
citation = "sway/sway/tree/workspace.c"
```

`contrib/coverage-report --check` validates the data and
`src/tests/i3_conformance.rs` runs it on every build. It rejects an
un-itemised skip, a skip with no reason or no citation, a duplicate or
out-of-range assertion number, a file whose classified assertions do not sum to
its declared plan, and a non-passing file with no stated reason.

`contrib/coverage-report` derives the vendored-file count, green-file count,
assertion total, finished classifications, and work queue directly from the
manifest. The validator enforces each file's arithmetic. Progress is the work
queue falling and the documented-skip count rising.

The 208 failures were measured by **running every affected file and parsing
its TAP output** (`contrib/tap-extract`), not by reading prose. All 26 measured
counts matched what the table had claimed, which is the first independent
confirmation those numbers were right.

Why this moved out of prose: counting the old table meant reverse-engineering
English -- literal `|` inside sway syntax (`focus next|prev`), rows stating a raw
TAP run *and* its superseding classification in one cell, rows counting subtests
and leaf assertions together. Ad-hoc parsing produced a fail total of **880**
when the real figure was **208**, and a green ceiling of **107** when it was
**105**. Always overstating our defects, which is the expensive direction: it
sends people hunting for bugs that are already explained.

Every reason code in `coverage.toml` was originally inferred by regex from this
table's old prose, and at least two were provably wrong:
`504-move-workspace-to-output.t` was tagged `bar_protocol` because its note
mentions *removing* an i3bar block, and `306-move-to-parent.t` was tagged
`compositor_restart` for the word "restart" in an unrelated sentence. The
validator now rejects every unverified reason.

`contrib/coverage-report --gaps` prints the whole work queue, largest first,
with each file's failures, unreached assertions and undocumented skips. That
list is all a worker needs; no prose reading is required to find or claim work.

### What `fail` is allowed to mean

The target state for this suite is **zero assertions labelled `fail`**. Green,
`skip` and `unproven` are all acceptable end states; `fail` is not.

That is not a relabelling exercise. The three labels answer different questions,
and the distinction is the whole value of this table:

- **`pass`** -- we asked, and got sway's answer.
- **`skip`** -- we asked, and the expectation itself is not sway's behaviour.
  Requires a citation to sway or i3 source. A skip is a *finding*: it says the
  test is wrong about sway, not that we gave up.
- **`unproven`** -- the assertion's **premise** never held, so its outcome
  carries no information. This is the label most easily abused, so it has a
  strict test: an assertion is unproven only when the setup it depends on did
  not happen. Two shapes qualify.

  *Never reached.* The file aborted earlier, so the assertion did not run at
  all. 143 assertions across 17 files.

  *Reached, but on a false premise.* The assertion ran and reported a failure,
  and the failure is meaningless because the state it was meant to check was
  never established. 39 assertions across 5 files: `005-floating.t`,
  `181-regress-float-border.t`, `189-floating-constraints.t`,
  `272-regress-focus-assign.t` and `293-focus-follows-mouse.t`. All five depend
  on i3's X11 absolute initial rectangle, which `xdg_toplevel` has no request
  for, so the window is never the size or position the assertion assumes.
  Checking a later property of a window that was never placed correctly tells
  you nothing about the compositor.

  **The second shape is the dangerous one**, because it looks identical to a
  defect in the raw TAP output: the run says `not ok`. The distinction is not
  the result but whether the premise held, and every such row must say which
  earlier assertion failed to establish it. A row that reclassifies a failure as
  unproven without naming the missing premise is hiding a bug.
- **`fail`** -- we asked on a sound premise and got the **wrong** answer. This
  is a defect in swayward, and it stays labelled `fail` until the code
  changes.

So a documented divergence must never be left as `fail`. If a file's prose
already explains why sway behaves differently and cites the source, the label
belongs in `skip`, and leaving it as `fail` overstates our defect count. The
inverse error is worse: relabelling a real defect as `skip` to clear the board
hides a bug behind a citation. A citation that does not actually justify the
divergence is worse than no citation at all.

`unproven` and `skip` are not interchangeable either. `skip` means the question
was wrong -- the test expects i3 behaviour that sway does not have, which is a
statement about the test. `unproven` means the question was never validly put --
a statement about our harness. Both are acceptable end states, but they point at
different owners: a skip is closed forever, while an unproven assertion would
become answerable if the harness gained the missing input.

Neither label may be applied to reduce a fail count. The honest summary of this
suite is that **207 assertions across 25 files are real failures**, and the
target of zero means fixing or correctly explaining each one, not moving it.

### Obsolete `mode toggle`

Eight vendored files use i3's obsolete `mode toggle` to float a window. Neither
the pinned i3 revision nor sway reads that as a floating command: both treat
`mode` as binding-mode selection, so it looks for a mode named `toggle` and
leaves the window tiled (`sway/sway/commands/mode.c:23-45`;
`i3/parser-specs/commands.spec:480-482`). The modern spelling is `floating
toggle`, so these files test less than their authors intended against sway as
well as against swayward.

In five the command is load-bearing, so their assertions stay unproven rather
than skipped -- the commands execute, but the state they were meant to set up
never exists:
`127-regress-floating-parent.t`, `142-regress-move-floating.t`,
`147-regress-floatingmove.t`, `148-regress-floatingmovews.t` and
`151-regress-float-size.t`. In the other three it is incidental to what the file
checks, and they pass in full: `126-regress-close.t`,
`144-regress-floating-resize.t` and `152-regress-level-up.t`.

The status `skip: i3-only tree structure` applies when a test requires the i3
output-level `content` container. Sway places workspaces directly below outputs
(`sway/sway/ipc-json.c:869-874`), so the adapter cannot expose that i3 node
without fabricating a tree that real sway clients do not see. See
[Known deviations from sway](../../docs/KNOWN_DEVIATIONS.md#output-content-nodes).

| File | Assertions | Status | Reason |
| --- | ---: | --- | --- |
| `120-multiple-cmds.t` | 31 | finished: 8 pass; 12 documented skip; 11 fail | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `122-split.t` | 31 reached; no TAP plan | 31 pass; remainder unproven after i3-only tree lookup | The fresh unchanged run emits no rejected commands and passes assertions 1–31, including singleton stacked assertions 28 and 30. It then aborts at line 157 while dereferencing i3's output-level `content` node, which sway does not have: sway places workspaces directly below outputs (`sway/sway/ipc-json.c:854-894`). The old row called the unexecuted remainder a skip, but the file emits neither a TAP skip nor a final plan. |
| `127-regress-floating-parent.t` | 4 | unproven | The file's two obsolete `mode toggle` commands cannot create and later restore the floating container whose parent-removal sequence it intends to test. At the pinned i3 revision, `mode` selects a binding mode (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`), while floating state uses `floating toggle` (`i3/parser-specs/commands.spec:260-262`). Sway agrees (`sway/commands/mode.c:23-80`). The unchanged assertions pass 4/4 but are not evidence for the intended regression; a temporary corrected-command diagnostic passes 4/4. |
| `133-size-hints.t` | 16 | finished: 0 pass; 16 skip | Every assertion depends on an ICCCM aspect-ratio hint installed by `open_with_aspect`; even the six literal width/height checks are outcomes of that relational constraint. Native xdg-toplevel exposes independent committed `min_size` and `max_size`, but no width:height relation, so all 16 assertions are skipped rather than approximating aspect ratio with bounds (`sway/sway/desktop/xdg_shell.c:149-157`; `smithay/src/wayland/shell/xdg/mod.rs`, `ToplevelCachedState`). Native real-client tests separately prove that committed min/max sizes clamp initial floating and runtime resize, zero clears each bound, and requests without a surface commit do not affect sizing. |
| `141-resize.t` | 84 | 80 raw pass; 4 fail; effective 79 pass and 5 classified skips | The fresh unchanged run reaches `1..84` with no TAP skips. Assertions 61, 70, and 77 fail because the adapter floats a full-output tiled window, which swayward clamps to sway's automatic output-layout maximum. Assertions 83-84 use a helper that descends through i3's floating wrapper, which sway omits (`sway/sway/ipc-json.c:475-484`): both old and new rectangles are absent, so assertion 83 passes vacuously and assertion 84 fails. A native four-edge test starts with a smaller float and proves the portable resize behavior. |
| `116-nestedcons.t` | 5 reached; no TAP plan | 4 pass; 1 fail; 2 later assertions blocked | The unchanged run has no rejected-command diagnostic. Assertion 1 fails because the expected i3 root schema includes fields that sway omits; the first reported difference is `scratchpad_state`. Assertions 2–5 pass. Workspace discovery then assumes i3's output-level `content` container and aborts before assertions 6–7. Sway's common node serializer stops before i3's extra root fields, and sway places workspaces directly below outputs (`sway/sway/ipc-json.c:258-296,838-874`). The former `4 pass; 3 skip` result was a classification, not the emitted TAP result. |
| `177-bar-config.t` | 50 | 50 unproven; deliberate no-bar deviation | The unchanged file reaches zero assertions because the adapter does not expose `get_bar_config`, and its later phases require `bar {}` configuration plus compositor lifecycle. Swayward deliberately does not retain bar settings or launch swaybar: preserving the full schema would serve no managed component, while stock Waybar is configured independently. Empty `GET_BAR_CONFIG` correctly returns `[]`, and a requested ID now returns sway's `No bar with that ID` error (`sway/sway/ipc-server.c:846-878`; `sway/sway/ipc-json.c:1271-1466`). |
| `317-bar-config-font-fallback.t` | 1 | unproven; deliberate no-bar deviation | Translation fails loud on the `bar {}` block. The assertion checks swaybar's fallback from an invalid global font, but swayward neither configures nor launches swaybar. Configure Waybar's font directly; see `177-bar-config.t`. |
| `317-bar-config-font-order.t` | 1 | unproven; deliberate no-bar deviation | Translation fails loud on the `bar {}` block. The assertion checks inheritance of the compositor-wide font into swaybar regardless of declaration order, a relationship absent when Waybar owns its configuration. See `177-bar-config.t`. |
| `317-bar-output-trailing-space.t` | 4 | unproven; deliberate no-bar deviation | Both phases stop at fail-loud `bar { output ... }` blocks. Trailing-space parsing matters only to the omitted bar-output list and its GET_BAR_CONFIG serialization, not any supported compositor setting. Swayward does not silently accept and discard the directive; configure Waybar's outputs directly. See `177-bar-config.t`. |
| `538-i3bar-primary-output.t` | 4 | unproven; deliberate no-bar deviation | Both phases stop at fail-loud `bar { output primary|nonprimary }` blocks. These selectors configure where swaybar runs; swayward manages no bar and therefore has no authoritative bar ID or output list to serialize. Configure Waybar's output selection directly. See `177-bar-config.t`. |
| `102-dock.t` | 23 | 1 reached; 0 pass; 23 classified skips | The unchanged file reaches its initial empty-dock assertion, but `get_dock_clients` is hardcoded empty because sway keeps layer-shell surfaces outside GET_TREE, so that baseline cannot fail and is not credited as a pass. The file then aborts at the missing X11 `screens` method before creating `_NET_WM_WINDOW_TYPE_DOCK` windows. Native layer-shell tests instead prove full-width top placement, cumulative exclusive-zone removal, and live height reconfiguration (`sway/sway/desktop/layer_shell.c:71-100`; `sway/sway/ipc-json.c:838-874`). |
| `264-dock-criteria.t` | 14 | 14 unproven; native layer-shell substitution | The unchanged file stops during config translation because its i3bar-disabling `bar` block is unsupported, and every assertion then requires an X dock and dockarea nodes. A native layer surface remains outside the view/container set: criteria return `No matching node` and cannot move, fullscreen, or kill it. Sway likewise matches criteria against tree nodes and views, while layer surfaces are managed separately (`sway/sway/criteria.c:168-190,500-522`; `sway/sway/desktop/layer_shell.c:79-100`). |
| `526-reconfigure-dock.t` | 3 | 3 unproven; native layer-shell substitution | The unchanged file stops at its i3bar-disabling `bar` block before creating an X dock. Native tests prove that a layer surface remains assigned to its requested output and accepts a new configured height. Reconfiguring size without changing the exclusive zone leaves the tiled working area unchanged; changing the zone updates it. Sway re-arranges tiling only when the usable area changes (`sway/sway/desktop/layer_shell.c:79-100`). The X ConfigureRequest and dockarea-node assertions remain unproven. |
| `155-floating-split-size.t` | 4 | unproven | The file depends on X11 client-requested `rect` sizes and expects `focus parent; floating enable` to float the whole split. The Wayland adapter cannot provide the requested geometry, and sway floats the selected parent container (`sway/commands/floating.c:23-55`); swayward’s focus-only tiling root has no window that its floating API can move. |
| `142-regress-move-floating.t` | 1 | unproven | The obsolete `mode toggle` command leaves the window tiled, so the liveness assertion does not test moving a floating window between workspaces. Both pinned i3 and sway treat `mode` as binding-mode selection rather than floating state (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`; `sway/commands/mode.c:23-80`). The unchanged assertion passes 1/1 but is not evidence for the intended regression; a temporary `floating toggle` diagnostic passes 1/1. |
| `147-regress-floatingmove.t` | 2 | unproven | The obsolete `mode toggle` command leaves the focused parent tiled, so neither liveness assertion tests movement out of a floating container. Both pinned i3 and sway treat `mode` as binding-mode selection rather than floating state (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`; `sway/commands/mode.c:23-80`). The unchanged assertions pass 2/2 but are not evidence for the intended regression; a temporary `floating toggle` diagnostic passes 2/2. |
| `151-regress-float-size.t` | 1 | unproven | The file's two obsolete `mode toggle` commands omit both the floating and return-to-tiling transitions, so its liveness assertion tests only opening two tiled windows. Both pinned i3 and sway treat `mode` as binding-mode selection rather than floating state (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`; `sway/commands/mode.c:23-80`). The unchanged assertion passes 1/1 but is not evidence for the intended regression; a temporary corrected-command diagnostic passes 1/1. |
| `117-workspace.t` | 92 | 50 reached; no TAP plan; remainder unproven | Negative workspace prefixes serialize as `num: -1`, matching sway (`sway/sway/ipc-json.c:503-517`). The unchanged file aborts at assertion 51 before `done_testing`: `workspace_numbers_sorted` assumes that every output has an i3 `content` child and dereferences the missing child. Sway places workspaces directly below outputs (`sway/sway/ipc-json.c:854-894`), so the harness cannot continue without replacing the test's tree traversal. The former 89 pass and 3 skip count came from a direct-workspace diagnostic, not the unchanged file. Later rename assertions remain unproven by this run; sway resolves names case-insensitively and returns success without changing case when the target resolves to the same workspace (`sway/sway/commands/rename.c:36-38,66-92`). |
| `176-workspace-baf.t` | 26 | unproven | The file launches two compositor configurations, changes `workspace_auto_back_and_forth`, and restarts i3. Rebuilding `Fixture` destroys its Wayland clients and windows; `State::reload_config` preserves them and is not a compositor restart. Substituting either lifecycle would change the test's input. |
| `174-border-config.t` | 13 | finished: 11 pass; 2 fail | The deprecated `new_window` and `new_float` aliases map to per-window initial border rules without changing swayward's shipped 4px default. Assertions 10 and 13 fail because they descend through i3's floating-wrapper child, while sway serializes the floating leaf directly (`sway/sway/ipc-json.c:478-484,532-540`). A direct-leaf diagnostic confirms the normal/2px and pixel/1px states. The previous row called these failures skips, but the unchanged run contains no TAP skips. |
| `181-regress-float-border.t` | 6 | 0 pass; 6 fail; classified unproven | The fresh unchanged run reaches `1..6`: all six assertions fail, no TAP skip is emitted, and `border 1pixel` is rejected. Its X11-requested 200×100 geometry is unavailable to native `xdg_toplevel`, so assertions 1–4 cannot establish the intended baseline. Assertions 5–6 compare that wrong initial floating rectangle with the output-sized fullscreen rectangle and therefore also fail; the old row incorrectly credited assertion 5 as a pass. The legacy border spelling is i3-only; sway accepts `border pixel 1` instead (`sway/commands/border.c:13-99`). Native tests cover the supported spelling and prove that changing border metadata does not resize floating content. |
| `185-scratchpad.t` | 2 reached; no TAP plan | 2 pass; remainder unproven | The fresh unchanged run emits no rejected commands, proves only that the root is first and synthetic `__i3` exists, then aborts at line 39 while looking for i3's output-level `content` child. Sway puts `__i3_scratch` directly below synthetic `__i3` and hidden scratchpad containers in its `floating_nodes` (`sway/sway/ipc-json.c:459-499`), so the unchanged traversal cannot continue. Later sections also require X11 client-requested geometry, restart persistence, and floating split containers. The old `16 pass` count came from a diagnostic, not the unchanged file. |
| `187-commands-parser.t` | 25 | skip: i3 parser internals | Every assertion invokes i3's standalone `test.commands_parser` binary and compares its generated-parser callback trace or exact diagnostic text. The harness has no such binary, so all 25 receive `command not found`. Sway dispatches commands through its own handler tables and `split_args` path rather than i3's generated parser (`sway/commands.c:151-176,250-329`); reproducing i3 callback names and diagnostics would not test sway-compatible runtime behavior. |
| `193-ipc-version.t` | 4 | finished: 3 pass; 1 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `210-mark-unmark.t` | 17 | finished: 14 pass; 3 fail | Test-side Wayland identity translation proves assertions 7–13 and 16. Assertions 14, 15, and 17 fail because they expect i3 to reject one mark applied to several matches. Sway instead runs the command for each match and moves the duplicate mark to the last container (`sway/sway/commands.c:301-326`; `sway/sway/commands/mark.c:46-58`). The translated `instance` criterion and node identity expose that last-match-wins result. The previous row called these failures skips, but the unchanged run contains no TAP skips. X11 identity remains unproven (`sway/sway/ipc-json.c:670-683`). |
| `260-invalid-criteria.t` | 2 | finished: 1 pass; 1 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `268-ipc-config.t` | 2 | 1 reached; 0 pass; 1 classified skip; 1 unreached | The only reached check proves that a newly allocated temporary pathname does not exist before launch. It runs before the unavailable separate-compositor setup, so it provides no GET_CONFIG or `ipc-socket` evidence. |
| `262-config-validation.t` | 2 | skip: i3 config language and diagnostics | The file invokes i3's `-C` validation through its launcher and expects an i3 parser token diagnostic. Swayward has a compositor-free `swayward validate --config PATH` command, verified with valid and invalid KDL, but translating the deliberately invalid i3 source cannot produce KDL to validate. Sway likewise implements compositor-free validation as `-C`/`--validate` (`sway/sway/main.c:206-220,248-249,351-359`) and restores the active config after validation (`sway/sway/config.c:446-475,510-514`). Substituting KDL or i3's diagnostic would change the test input or oracle. |
| `235-check-config-no-x.t` | 8 | skip: i3 executable, config language, and diagnostics | Every assertion shells out directly to `DISPLAY= i3 -C`, feeds i3 syntax, and checks i3 exit codes or parser and duplicate-binding text. Swayward's equivalent is `swayward validate --config PATH`; it runs without a display and returns 1 for invalid KDL and 0 for valid KDL. Sway documents and dispatches `-C`/`--validate` without starting the compositor (`sway/sway/main.c:206-220,248-249,351-359`). Replacing the executable, syntax, and expected diagnostics inside the byte-identical file is not a conformance test. |
| `196-randr-output-names.t` | 1 | skip: i3 executable and config language | Despite its name, the file only shells out to `i3 -C` with an i3 `workspace 2 output DVI-I_1/digital` directive and checks i3's output for an error. It does not create or query a RandR output. Swayward's compositor-free validator consumes KDL, and the translator already handles workspace output preferences. Sway documents and dispatches `-C`/`--validate` without starting the compositor (`sway/sway/main.c:206-220,248-249,351-359`). |
| `298-ipc-misbehaving-connection.t` | 2 | finished: 1 pass; 1 fail with native substitute | The unchanged file's 500 workspace switches complete without a rejected command or compositor stall after the adapter removes only i3's `ipc_kill_timeout 500`; sway has no timeout setting and instead queues nonblocking writes from an initial 128-byte allocation, doubles the allocation, and disconnects when the new size exceeds 4 MB (`sway/sway/ipc-server.c:172-197,524-568,946-964`). Those small workspace events do not exceed sway's threshold, so assertion 2 fails rather than proving sway's fixed-size policy. Native IPC coverage drives the real subscribed non-reading socket past the 4 MiB power-of-two boundary, verifies a separate GET_VERSION request while the socket is backpressured, and observes EOF. The previous row incorrectly called the failing assertion unproven. |
| `301-shape.t` | 4 | 0 reached; no TAP plan; 4 unproven; native input-region substitute passes | Each assertion depends on both X Shape kinds: the test applies a 100×50 bounding shape, which removes the lower half visually and from input, and a 50×50 input shape, which makes the upper-right quarter click through. Native Wayland splits those mechanisms: `wl_surface.set_input_region` controls input only, while buffer alpha controls rendering; no single xdg-toplevel request reproduces X bounding shape (`i3/testcases/t/301-shape.t:48-59`; `smithay/src/wayland/compositor/handlers.rs:240-266`). The fresh unchanged file emits no rejected-command diagnostic and stops before assertions because its Inline C XCB Shape setup has no X server in the Wayland harness; all four assertions remain unproven. Native real-client coverage commits partial and empty non-null input regions, drives real pointer motion and buttons, identifies the top or lower receiver in both directions, preserves border/titlebar activation, and distinguishes empty from the null full-surface default. |
| `307-focus-next-prev.t` | 9 | finished: 8 pass; 1 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `308-focus_wrapping.t` | 32 | 29 pass; 3 fail; random subtest contaminated by invalid i3 input | The fresh unchanged run fails assertions 14, 23, and 31: each expects focus to enter configured workspace `right-top`, but `focused_ws` reports `1`. The tree-shape portions of those subtests pass; the failures expose the deferred per-output startup-workspace model, not focus wrapping. Sway assigns configured workspace names to their requested outputs and otherwise derives each output's initial workspace before sorting it (`sway/sway/tree/workspace.c:153-182,177-267,334-490`). The remaining wrapping checks pass. The final random subtest is still contaminated by upstream's `qw(left right top down)` typo at line 332: both i3 and sway accept only `left|right|up|down` (`i3/parser-specs/commands.spec:185-193`; `sway/sway/commands/focus.c:439-449`). Every sampled `focus top` is rejected, so its subsequent no-change assertion is vacuous. The old row's 32/32 result is stale. |
| `322-match-error-crash.t` | 2 | 0 reached; no TAP plan; 2 unproven | The fresh unchanged run emits no rejected-command diagnostic. Configuration translation fails loud on three X11-only `window_type` criteria and one unsupported `no_focus` form, so neither the pre-reload nor post-reload liveness assertion runs. Native Wayland clients cannot provide X11 window type; the runner backtrace reports the fail-loud adapter boundary, not a compositor panic. Native coverage instead passes a config rejected for sway's supported malformed `con_id` criterion through the real watcher result boundary into `State::reload_config`, then proves the compositor still accepts commands. This answers the protocol-independent malformed-criteria crash question but does not prove either unchanged assertion. |
| `319-gaps.t` | 28 | finished: 23 pass; 3 documented skip; 2 fail | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `325-layout-percent-and-marks.t` | 3 reached; no TAP plan | 2 pass; 1 fail; 5 later assertions blocked | The unchanged file passes its empty-workspace and post-command liveness checks around a rejected `append_layout`, then fails the node-count assertion and aborts while dereferencing the absent restored container. It never reaches `done_testing`; the former `finished: 2 pass; 6 skip` result was wrong. All six intended shape, 0.2/0.8 percentage, and mark assertions depend on i3's JSON layout restoration engine. Sway does not implement `append_layout`; the command is absent from its complete config and runtime command tables (`sway/sway/commands.c:41-143`). The two passes do not prove restoration. |
| `321-crash-criteria-scratchpad.t` | 6 | 4 pass; 2 fail; no panic | All five criteria commands reach command dispatch against a real hidden scratchpad window, and the final liveness query proves only that the compositor survives. `nop`, directional focus, and `focus output left` succeed without revealing the window. Sway dispatches once per match, but `focus output` deliberately ignores that matched container: it resolves names first, otherwise resolves and wraps directions from the seat-focused workspace, and succeeds even when no output exists (`sway/sway/commands.c:301-326`; `sway/sway/commands/focus.c:310-352`). Native tests cover name and wrapped-direction selection with two outputs plus success-with-no-op on one output. Both floating commands are rejected with sway's expected `Can't change floating on hidden scratchpad container` failure (`sway/sway/commands/floating.c:35-38`); their unchanged state checks are nested inside the two failing subtests and do not prove command success. |
| `513-move-workspace.t` | 6 | finished: 4 pass; 2 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `527-focus-fallback.t` | 2 | finished: 1 pass; 1 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `528-workspace-next-prev-reversed.t` | 38 | 16 pass; 22 fail: mixed deferred-model and i3-only oracle | All assertions run without rejected commands. The initial numeric failures expose swayward's deferred creation and ordering model. The later `8:*` expectations are i3-only: sway parses every `8:*` name as number 8, and global next/prev select only strictly greater or smaller numbers, so they do not visit equal-prefix workspaces consecutively. Sway scans stored per-output lists only to break ties and to order nonnumeric names (`sway/sway/tree/workspace.c:540-677`). See [Workspace index and placeholder site mapping](../../docs/specs/2026-09-16-workspace-site-mapping.md#global-relative-navigation-is-not-direct-stored-order-traversal). |
| `535-workspace-next-prev.t` | 38 | 16 pass; 22 fail: mixed deferred-model and i3-only oracle | This complementary output order has the same split as `528`. The initial numeric failures depend on the deferred startup and ordering model. The later expectation that global navigation visits all equal-prefix `8:*` workspaces is i3-only. Sway uses strict numeric comparisons for global next/prev and stored-list order only for equal-choice tie breaking and nonnumeric traversal (`sway/sway/tree/workspace.c:540-677`). All assertions run without rejected commands. |
| `297-assign-workspace-to-output.t` | Later sections need multi-output assignment lists; the file declares no plan and aborts after 9. |
| `503-workspace.t` | 18 | 2 pass; 16 fail: deferred workspace model | Two real fake outputs exist and no command is rejected. The first failure proves the second output starts on `1` instead of sway's lazily derived next free workspace `2`; all global and per-output navigation checks then observe swayward's unsorted creation order. Sway derives one initial name per output from assignments, then bindings, then the first free number and sorts each output at insertion (`sway/sway/tree/workspace.c:334-490,255-259`). This is the documented deferred workspace identity/lifecycle model, not a parser defect. |
| `518-interpret-workspace-numbers.t` | 4 | finished: 3 pass; 1 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `320-mouse-bindings.t` | 13 | finished: 11 pass; 2 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `316-drag-container.t` | 58 inner assertions across 9 subtests | finished: 9 pass; 8 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `534-dont-warp.t` | 2 | finished: 0 pass; 2 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `512-move-wraps.t` | 10 | finished: 8 pass; 2 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `245-move-position-mouse.t` | 8 | finished: 6 pass; 2 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `504-move-workspace-to-output.t` | 31 | 10 pass; 21 fail; direct-node diagnostic: 24 pass, 7 fail | The adapter removes the exact test-only block that disables i3bar, then creates the file's two real fake outputs. The unchanged file reaches every assertion. Twenty-one checks use i3's output `content` wrapper, which sway omits. A direct-workspace diagnostic passes 24 assertions and leaves seven deferred-model or hierarchy failures: the second output starts on `1` instead of `2`, moving the last source workspace creates `1` instead of i3's `3`, and dependent membership checks inherit those identities. Named, directional, optional-`output`, and criteria-targeted workspace moves are covered natively, including adjacent-only direction lookup from the moved workspace's output, failure at an output edge, matched-window workspace context, and preserved floating-window coordinates. The unchanged file still expects i3's directional wrapping, while sway returns failure when no adjacent output exists (`sway/sway/commands/move.c:630-665`; `sway/sway/tree/workspace.c:436-490`). |
| `266-net-moveresize-window.t` | 12 | 0 reached; no TAP plan; 12 classified as i3-only | The unchanged file stops before assertions when its first raw X11 client message fails loud. i3 implements `_NET_MOVERESIZE_WINDOW` by converting its position and size flags into an X11 configure request (`i3/src/handlers.c:960-988`) and advertises the atom (`i3/include/i3-atoms_NET_SUPPORTED.xmacro.h:39`). A complete search finds no atom handler or mention in sway's source or command reference, so sway does not implement this i3 behavior even for its in-process Xwayland clients. Swayward's `xwayland-satellite` owns X11 window-manager messages and presents ordinary `xdg_toplevel` surfaces; that boundary cannot carry this absolute request to swayward. As documented for `112-floating-resize.t`, `xdg_surface.set_window_geometry` is surface-local, while `xdg_toplevel.move` and `resize` are serial-gated interactive requests without target geometry. All twelve unexecuted assertions are therefore classified as i3-only rather than treated as unproven sway behavior; the unchanged file emits no TAP skips. The fail-loud `send_event` stub remains unchanged. |
| `291-swap.t` | 11 reached; no TAP plan | 11 pass; remainder unproven | The fresh unchanged run reaches one liveness check, one cross-workspace `con_id` result, and nine same-workspace mark-swap shape/focus assertions. The initial `swap container with con_id 1` is rejected as expected; its `does_i3_live` pass proves only that the invalid request did not crash. Map-time `mark` rules translate through the existing `sway-for-window-command` path and assign globally unique marks (`sway/sway/commands/mark.c:10-61`). The file then aborts at line 201 because the native client has no X11 `fullscreen` method. Later fullscreen permutations, X11 `id` targets, and floating rectangles remain unproven: sway's `id` swap branch exists only with Xwayland, while `con_id` and mark are portable (`sway/sway/commands/swap.c:13-93`). The old row undercounted the fresh passes by one. |
| `289-ipc-shutdown-event.t` | 4 | finished: 0 pass; 4 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `271-for_window_tilingfloating.t` | 20 | 0 reached; no TAP plan; 12 classified as i3-only, 8 later assertions blocked | Of the six criteria, two are sway-supported current-state predicates: `tiling` and `floating` (`sway/sway/criteria.c:607-611`; `sway/sway/sway.5.scd:1044-1045,1068-1069`). The translator maps them exactly to KDL `is-floating=false|true`, and now preserves all valid `mark` flag combinations through the existing map-time command surface. Four are i3-only provenance predicates: `tiling_from=auto|user` and `floating_from=auto|user` (`i3/src/match.c:417-443`). A complete search of sway source and runtime documentation finds no provenance criterion, so those forms remain fail-loud and their 12 dependent assertions are classified as i3-only rather than conflating “was tiled or floating by policy” with current state. The unchanged file emits no TAP or rejected-command diagnostic because translation stops before launch. The eight current-state assertions remain unproven because this adapter's `open_floating_window` maps a native toplevel tiled and changes it afterward, unlike upstream's pre-map X11 utility type; native tests prove tiled-state map-time marks and global mark uniqueness. |

## Coverage table

| File | Assertions | Result | Notes |
| --- | ---: | --- | --- |
| `100-fullscreen.t` | 79 | 72 pass; 7 fail classified as skips | The fresh unchanged run reaches `1..79`. Assertions 7-8 and 14-15 fail because the file locates its workspace beneath i3's output-level `con` content node, while sway serializes workspaces directly below outputs (`sway/sway/ipc-json.c:459-499`); the adapter no longer fabricates that node. Assertions 16 and 33 depend on i3 withholding an X11 map while another window is fullscreen, and assertion 41 depends on the adapter accepting `fullscreen => 1` during `open_window`. The other 72 assertions pass against the sway-shaped tree. |
| `124-move.t` | 54 | 49 pass; 5 fail; classified 5 skip | Assertions 1–20 pass: mapping after `focus parent` resolves the focused split to its inactive view and inserts the new window as that split's sibling, matching sway (`sway/tree/view.c:850-901`); nested directional moves then descend, promote, and compact correctly (`sway/commands/move.c:112-163,300-413`). Assertions 21–40 pass with sway's 10 px default and optional custom pixel distance (`sway/commands/move.c:672-710`). Assertion 41 expects directional percentage-point movement. Sway accepts only an optional pixel amount for directional moves and returns `Invalid distance specified` for `ppt` (`sway/sway/commands/move.c:672-681`), so the assertion is i3-only. Swayward currently accepts the command as a 25-pixel move instead of returning sway's failure; this separate command-result mismatch does not make the i3 geometry assertion portable. Assertions 45–50 pass for pixel, workspace-relative percentage, and absolute-center positions (`sway/commands/move.c:784-918`). Assertions 51–54 fail because the unchanged lookup descends through i3's floating wrapper, yields empty `con_id` values, and both criteria commands are rejected before moving either direct sway floating leaf. They are classified under the [i3-only floating-wrapper limit](#floating-wrappers-are-i3-only); a direct-node diagnostic proves the criteria-targeted position behavior. The former row incorrectly described all five as emitted TAP skips and incorrectly said sway treated `25 ppt` as 25 pixels. |
| `156-fullscreen-focus.t` | 64 | unchanged TAP: 55 pass; 9 fail; classified 55 pass, 9 skip | Assertions 16–17 expect i3 to wrap inside a global-fullscreen subtree. Sway instead returns at the global-fullscreen barrier before recording a wrap candidate (`sway/commands/focus.c:139-156,178-223`). Assertions 37–39 expect directional moves within the workspace-fullscreen subtree to leave its i3 shape unchanged; sway can move a descendant before its ancestor walk reaches the fullscreen node (`sway/commands/move.c:301-413`). Those mutations contaminate assertions 40–43. Independently, sway allows cross-workspace moves under workspace fullscreen but rejects them under global fullscreen with `Can't move fullscreen global container` (`sway/commands/move.c:419-441,994-1024`). The two rejected global moves are no-change false positives against an already changed shape, not evidence. These nine failures remain classified as i3-only skips, but the unchanged file emits failures, not TAP skips. The other 55 assertions pass (`sway/commands/focus.c:88-223,405-412`; `sway/tree/container.c:587-605,1200-1339`). |
| `164-kill-win-vs-client.t` | 12 | finished: 11 pass; 1 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `165-for_window.t` | 0 | unproven | The constant-only `X11::XCB` stub now lets the unchanged file compile and reach configuration translation, where it fails loud before assertions on 16 unsupported directives. Portable border actions and combined class-plus-title criteria translate. Remaining blockers are X11-only `instance`, `id`, `window_role`, and generated `window_type` criteria, plus missing workspace criteria and map-time `mark`/`exec` actions. |
| `167-workspace_layout.t` | 5 reached; no TAP plan | 5 pass; 83 later assertions skipped as i3-only setup | The unchanged file aborts before its second phase because it uses i3's `workspace_layout stacked` spelling. Sway accepts only case-insensitive `default|stacking|tabbed` and rejects `stacked` (`sway/commands/workspace_layout.c:5-20`; `sway/sway.5.scd:92-93`), so swayward keeps the setup fail-loud. The translator maps all three exact sway values to a dedicated KDL setting, and native tests prove that `stacking` and `tabbed` wrap each workspace-level insertion through sway's default-layout path (`sway/tree/workspace.c:939-958,985-994`). A temporary corrected-spelling diagnostic reached 87 assertions: 57 passed and 30 exposed separate move, empty-workspace layout, and layout-command differences. The unchanged run does not emit 83 TAP skips or a terminal plan. |
| `166-assign.t` | 106 | finished: 99 pass; 7 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `169-border-toggle.t` | 20 | finished: 18 pass; 2 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `189-floating-constraints.t` | 28 | 20 reached: 10 pass, 10 fail; classified 8 pass, 20 unproven | Configuration translation preserves minimum and maximum values, including `-1` (unlimited) and `0` (automatic), and the runtime applies global limits before client hints (`sway/sway/commands/floating_minmax_size.c:9-52`; `sway/sway/tree/container.c:793-830`; `sway/sway/input/seatop_resize_floating.c:81-105`). Assertions 1–12 require unavailable X11 initial rectangles; 7–8 happen to pass only because the adapter's default window is below 2048 and therefore remain unproven. Assertions 13–14 prove portable command growth reaches the configured maximum. Assertions 15–16 prove that a rejected further growth leaves position unchanged: sway returns `Cannot resize any further` at the maximum (`sway/sway/commands/resize.c:192-222`), and the rejection is on the allow-list. Assertions 17–20 prove that unitless `resize set` values are pixels for floating containers and that both configured clamps apply. Assertions 21–28 require X11 `WM_NORMAL_HINTS`; the fail-loud `before_map` boundary stops after assertion 20. Native `xdg_toplevel.set_min_size` and `set_max_size` are protocol-equivalent size hints but remain unavailable in this harness. The former classification wrongly counted accidental assertions 7–8 as evidence. |
| `228-border-widths.t` | 21 | finished: 12 pass; 9 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `236-floating-focus-raise.t` | 6 | unchanged TAP: 0 pass; 6 fail; classified 6 skip | Every assertion descends through an i3 `floating_con` child, so each lookup returns undefined against sway's direct floating leaves. Sway serializes each floating container directly in `floating_nodes` and raises a directionally focused float to the end of that list (`sway/ipc-json.c:478-484,532-540`; `sway/commands/focus.c:475-486`; `sway/tree/container.c:1682-1692`). A direct-node diagnostic passes 6/6, and native mutation-verified coverage proves the order change. The six unchanged assertions are i3-only, but they emit failures rather than TAP skips. |
| `005-floating.t` | 13 | finished: 6 pass; 7 fail, all 7 unproven | The unchanged run emits no rejected-command diagnostic or TAP skips. Assertions 5 and 7–13 fail because they depend on i3's X11 `rect` creation input. Wayland `xdg_toplevel` has no equivalent absolute-position request; using a window rule or compositor move would test a different input. Sway clamps and centers natural floating geometry (`sway/tree/container.c:793-905,955-982`). See the adapter limitation above. The former row classified these as unproven without recording that the unchanged test emits seven failures. |
| `135-floating-focus.t` | 82 | unchanged TAP: 69 pass; 13 fail; classified 69 pass, 13 skip | Assertions 23–25 require distinct X11 positions that native xdg-toplevel cannot request. The other ten failures descend through i3's `floating_con` wrapper; sway serializes direct floating leaves instead (`i3/src/floating.c:232-351`; `sway/ipc-json.c:478-484,532-540`). Direct-node checks pass where the hierarchy agrees. Layer focus modes, workspace child descent, cross-workspace focus, nested reinsertion, and close restoration match sway. Assertions 75 and 77 pass through a direct-node diagnostic after preserving non-root parents across floating transitions (`sway/tree/container.c:955-1013`). The direct-node order also follows sway, which appends new floating containers (`sway/tree/workspace.c:961-971`). The 13 exclusions remain valid, but the unchanged file emits failures rather than TAP skips. |
| `112-floating-resize.t` | 11 | 0 reached; no TAP plan; 11 unproven | The unchanged file stops at its first `Window::rect(Rect)` call with `X11 window geometry mutation is unavailable in the Wayland test adapter`. It emits no TAP result or plan. Assertions 1–9 depend on X11 configure requests that set a floating window's position and size; assertions 10–11 depend on an out-of-bounds position request. Native `xdg_toplevel` has no equivalent request: `set_window_geometry` is surface-local and must not be treated as desktop placement, while `move` and `resize` are serial-gated interactive operations without target coordinates or dimensions (`xdg-shell.xml`, `xdg_surface.set_window_geometry`, `xdg_toplevel.move`, `xdg_toplevel.resize`). Client buffer resizing after `ack_configure` would answer a different question. All 11 assertions remain unproven, and the fail-loud setter is retained. |
| `148-regress-floatingmovews.t` | 1 | unproven | The unchanged file uses obsolete `mode toggle`, which both pinned i3 and sway interpret as binding-mode selection rather than floating state (`i3/parser-specs/commands.spec:480-482`; `sway/commands/mode.c:23-62`). The rejected setup leaves the second window tiled. A temporary `floating toggle` diagnostic still fails because moving the focused float away restores focus to the prior tiled window, matching sway's focus-restoration path (`sway/commands/move.c:554-608`); the assertion's expected remote focus is i3-only. |
| `153-floating-originalsize.t` | 7 | unproven | All assertions depend on `open_window(rect => ...)`, an X11 client size request that the Wayland adapter cannot reproduce. The first three observe the adapter's 1×1 default stretched by tiling, and the final four compare the float against the unavailable 400×150 request, so none proves original-size restoration. |
| `139-ws-numbers.t` | 8 | 3 pass; 5 fail: deferred workspace model | All commands parse. Number serialization and lookup pass, but workspaces remain in creation order instead of sway's numeric-first insertion order (`sway/sway/tree/workspace.c:255-259`). This is the already-documented workspace identity/lifecycle design issue, not a new defect. |
| `254-move-to-output-with-criteria.t` | 16 | finished: 14 pass; 2 fail | `fresh_workspace(output => N)` selects each real output through sway's `focus output <direction|name>` command. Assertions 12 and 14 fail because they expect i3 to cycle criteria matches across the supplied output list. Sway instead accepts the extra arguments but resolves only the first output name for every match (`sway/commands/move.c:419-425,519-525`), placing both windows on `fake-1`. The previous row called assertions 14 and 16 skips; the unchanged run emits no TAP skips, and assertions 16 and 12 respectively prove the empty old destination and the extra window on `fake-1`. |
| `287-edge-borders.t` | 31 | finished: 29 pass; 2 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `251-command-criteria-focused.t` | 2 reached; no TAP plan | 2 pass; 9 later assertions blocked | The unchanged run has no rejected-command diagnostic. Class-to-`app_id` substitution proves the initial count and `class=__focused__` movement, then the adapter fails loud when the next phase requests an X11 instance distinct from class. Native `xdg_toplevel` has only `app_id` (`xdg-shell.xml`, `xdg_toplevel.set_app_id`). The run never reaches the title, window-role, workspace, or no-focused-window phases; the former `finished: 9 pass; 2 unproven` result came from a diagnostic path rather than the unchanged file. Window role is separately X11-only: sway evaluates `instance` and `window_role` through its Xwayland criteria path and emits them only for Xwayland views (`sway/criteria.c:355-410`; `sway/sway/ipc-json.c:670-700`). |
| `293-focus-follows-mouse.t` | 10 | finished: 4 pass; 6 fail; classified 1 skip and 5 unproven | The unchanged run emits no rejected-command diagnostic and no TAP skips. Assertions 1 and 3–5 pass with real pointer motion and rendered hit testing. Assertion 2 fails because `(0,0)` hits the workspace rather than a tiled view; sway deliberately leaves focus unchanged when a workspace gap on the already focused output is hovered (`sway/sway/input/seatop_default.c:574-583`), so its i3 expectation is classified as skipped. A temporary diagnostic at `(20,30)`, inside the first rendered tile, makes assertion 2 pass. Assertions 6, 7, 9, and 10 fail after traversing i3 floating wrappers, which sway omits. Assertion 8 fails because the unavailable X11 `rect` requests were meant to place the overlapping floats at 1,1 and 50,50; the native clients retain compositor-chosen geometry, so `(40,40)` hits the other float. Those five failures remain unproven rather than focus-follows-mouse defects. |
| `297-assign-workspace-to-output.t` | 9 reached; no TAP plan | 2 pass; 7 fail; 16 later assertions adapter-blocked | The first child TAP stream reaches nine checks, then translation refuses seven ordered output-fallback directives before `done_testing`, so the file emits no plan. Counts that include panic diagnostics or repeated TAP streams are not assertion counts. Direct `special` assignment and binding-derived `bindingname` placement pass. The other seven startup checks expose the deferred workspace model: sway chooses one initial workspace per output from valid assignments, then bindings, then the first free number (`sway/sway/tree/workspace.c:334-490`), while swayward creates configured workspaces eagerly and does not derive missing initial names per output. The second phase is blocked before 16 assertions because the KDL workspace model rejects repeated names, while sway merges repeated `workspace <name> output ...` directives into one ordered output preference list (`sway/sway/commands/workspace.c:13-29,135-162`). The adapter helper reads output membership directly from `GET_WORKSPACES`; it does not fabricate tree nodes or outputs. The extra passing startup assertion came from the already-landed binding-derived initial workspace work, not fallback-list support. Exact support belongs to the identity, lifecycle, and assignment model in `docs/specs/2026-09-16-workspace-order-model.md`. |
| `294-focus-order.t` | 61 | finished: 53 pass; 8 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `232-cmd-move-criteria.t` | 22 | 12 pass; 10 fail; only 2 passes evidentiary | The portable `id` movement and focus-preservation assertions pass. Each of the ten `window_type` commands is rejected with `No matching node.`, each movement assertion fails, and each following focus assertion passes only because the rejected command changed nothing. Those ten passes are not evidence for criteria execution. Native Wayland clients cannot set X11 window types; sway evaluates `window_type` only in its Xwayland criteria branch (`sway/sway/criteria.c:355-410`). The old `12 pass; 10 unproven` total counted all ten vacuous focus checks as useful passes. |
| `243-move-to-mark.t` | 11 reached; no TAP plan | 11 pass; remainder unproven | The fresh unchanged run passes same-workspace reorder, missing-mark failure, and cross-workspace marked-leaf insertion, then aborts at its X11 `_NET_WM_STATE_DEMANDS_ATTENTION` message before assertion 12. Moving to a marked leaf inserts the source as its next sibling, while moving to a marked split appends it as a child (`sway/sway/commands/move.c:241-270,419-625,956`). Later wrapper and workspace-mark assertions do not run; sway rejects workspace marks because `mark` requires a container (`sway/sway/commands/mark.c:10-23`). The old `43 pass; 6 skip; 1 unproven` count came from a diagnostic and does not describe the unchanged file, which emits no TAP skips or final plan. |
| `194-regress-floating-size.t` | 15 | finished: 7 pass; 8 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `211-regress-urgency-assign.t` | 3 | finished: 1 pass; 2 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `218-regress-floating-split.t` | 2 | finished: 2 pass | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `206-fullscreen-scratchpad.t` | 5 reached; no TAP plan | 4 pass; 1 fail; 3 later assertions blocked | The unchanged run has no rejected-command diagnostic. Assertions 1–2 and 4–5 pass. Assertion 3 fails because it needs i3's floating wrapper: after `scratchpad show`, `floating toggle` addresses the child but i3 keeps focus on its floating parent, so `layout tabbed; focus parent; fullscreen` acts on that parent. Sway likewise promotes children of a floating split to its root (`sway/commands/floating.c:23-55`) and rejects layout changes on floating containers (`sway/commands/layout.c:119-125`). Swayward has no floating container node, so it cannot select or fullscreen that absent parent. After assertion 5, the file dereferences i3's synthetic `__i3_scratch` workspace, which sway's tree does not expose in that shape, and aborts before assertions 6–8. The former `7 pass; 1 unproven` count came from a diagnostic rather than the unchanged file. |
| `213-layout-restore-simple.t` | 18 | finished: 8 pass; 10 fail, all 10 classified skip | The unchanged run emits four expected `append_layout` rejections and no TAP skips. All ten failed layout-shape and swallow assertions depend on i3's `append_layout`, which sway does not implement: it is absent from sway's complete command tables and runtime command reference (`sway/sway/commands.c:44-144`; `sway/sway/sway.5.scd:102-415`). The eight empty-state and liveness assertions pass around rejected setup and do not prove layout restoration. The former `8 pass; 10 skip` result did not distinguish emitted failures from the cited classification. See [Layout restoration](../../docs/KNOWN_DEVIATIONS.md#layout-restoration). |
| `272-regress-focus-assign.t` | 8 | finished: 7 pass; 1 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `279-regress-default-floating-border.t` | 1 | 0 pass; 1 fail, classified as i3-only | The fresh unchanged run reaches `1..1`, emits no TAP skip or rejected-command diagnostic, and fails because the i3-only wrapper child is absent. The assertion is covered by the [i3-only floating-wrapper limit](#floating-wrappers-are-i3-only). Translation accepts deprecated `new_window pixel 5` and `new_float normal` as separate defaults. Native GET_TREE coverage proves that an initially tiled window reports `pixel`/5 while an initially floating window reports `normal`/2, matching sway's separate configuration fields and view-state selection (`sway/sway/config.c:305-306`; `sway/sway/tree/view.c:905-915`). Removing the floating override makes the native test report `pixel` instead of `normal`. |
| `280-wm-class-change-handler.t` | 4 | unproven | The unchanged file reaches zero assertions because translation correctly refuses the map-time `mark` action. Even with that setup bypassed, every assertion depends on changing X11 `WM_CLASS` after map and reading separate `class` and `instance` fields. Sway handles that only in its Xwayland `set_class` callback and reevaluates criteria there (`sway/sway/desktop/xwayland.c:695-706`); native xdg-shell exposes one mutable `app_id` instead (`sway/sway/desktop/xdg_shell.c:360-367`), so the adapter must not map this operation onto `app_id` or fabricate the two X11 properties. |
| `277-ipc-window-urgent.t` | 6 | unproven | The unchanged file enters its first subtest but executes zero real assertions before `Window::add_hint('urgency')` fails loudly. Native `xdg_toplevel` has no X11 `WM_HINTS` urgency bit, and sway handles that request only in its Xwayland view path (`sway/tree/view.c:1197-1224`). The former hard-coded GET_TREE urgency bug could not have been caught by this file in the native harness because neither event-producing operation runs. Real serial-less xdg-activation coverage remains in `240-focus-on-window-activation.t`. |
| `113-urgent.t` | 50 | 1 reached; no TAP plan; 1 pass, 49 later assertions blocked | The fresh unchanged run emits no rejected-command diagnostic. `force_display_urgency_hint 0ms` now translates to `urgent-timeout-ms 0`, and the unchanged file reaches its initial non-urgent check. It then stops at the first X11 `WM_HINTS` urgency request. Every urgency transition still requires either `WM_HINTS` or an X11 `_NET_WM_STATE_DEMANDS_ATTENTION` client message, which native Wayland cannot send. Native tests separately prove that focusing an urgent window clears it immediately, that a configured cross-workspace timeout delays the clear without restarting, and that unmapping cancels the timer, matching sway (`sway/sway/input/seat.c:1223-1240`; `sway/sway/tree/view.c:976-979`). Sway implements the runtime `urgent` command separately with boolean/toggle plus `allow` and `deny` forms (`sway/sway/commands/urgent.c:9-31`), but the file does not use that command to create urgency. |
| `212-assign-urgency.t` | 3 | finished: 2 pass; 1 fail, classified skip with portability substitution | The unchanged run emits no rejected-command diagnostic or TAP skips. Class-to-`app_id` assignment works, and windows assigned to a visible workspace on the current or another output leave it non-urgent. Assertion 1 fails because it expects i3's explicit policy of marking an assigned window urgent when its target workspace is invisible (`i3/src/manage.c:288-316`). Sway selects the assigned workspace and declines focus when it is not active, but never calls `view_set_urgent` from its map path (`sway/sway/tree/view.c:628-665,696-732,930-969`). Swayward follows sway, so the failure is classified as an i3-only skip. |
| `200-urgency-timer.t` | 11 | 0 reached; no TAP plan; 11 unproven | The fresh unchanged run emits no rejected-command diagnostic. `force_display_urgency_hint 500ms` now translates to `urgent-timeout-ms 500`, so configuration no longer blocks the unchanged file. It still stops before assertions at the first X11 `WM_HINTS` urgency request. Sway parses `<timeout> [ms]` into `config->urgent_timeout` and delays clearing urgency after a cross-workspace focus (`sway/sway/commands/force_display_urgency_hint.c:5-28`; `sway/sway/input/seat.c:1225-1239`; `sway/sway/sway.5.scd:763-768`). Native tests now cover immediate clearing, delayed cross-workspace clearing without timer restart, and timer cancellation on unmap. The remaining assertions require X11 `WM_HINTS`, which the adapter must not fabricate. |
| `312-regress-layout-default.t` | 0 | unproven | Upstream contains no TAP assertions. The adapter's zero-assertion guard rejects the file, so it cannot provide evidence even though both commands execute. |
| `510-focus-across-outputs.t` | 19 | finished: 11 pass; 8 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `517-regress-move-direction-ipc.t` | 2 subtests (6 inner assertions) | skip: i3-only workspace event | Real sway 1.11 emits no workspace event when `move right` carries the focused window to an adjacent output, whether the destination workspace is empty or occupied and whether the moved window is the source workspace's last window. Captured ordered streams are empty in all three cases. Sway reparents the container without calling `seat_set_focus`, so `set_workspace` cannot emit the focus event that i3 expects (`sway/commands/move.c:168-190,276-299,672-744`; `sway/input/seat.c:1098-1113`). |
| `544-focus-multiple-outputs.t` | 41 | unchanged TAP: 8 pass; 33 fail; classified 4 pass, 37 skip | Assertions 1, 10, 20, and 31 are direct setup checks and pass. Assertions 5, 9, 14, and 18 are rejected-command no-change false positives: focus happens to remain on the expected output during an intended cycle. The other 33 cycle assertions fail. Sway joins all arguments into one identifier, accepts only one output name or identifier or one direction, and rejects `next`, `nonprimary`, and output lists (`sway/commands/focus.c:310-352`; `sway/desktop/output.c:42-63`). All 37 cycle checks remain classified as i3-only skips, but the unchanged file emits 33 failures and no TAP skips. |
| `143-regress-floating-restart.t` | 5 | unproven | The unchanged assertions pass around a rejected `restart`, so no compositor state crosses an in-place restart. The harness cannot reproduce that lifecycle. |
| `150-regress-dock-restart.t` | 11 | unproven | The test combines X11 EWMH dock windows with in-place restart. The Wayland harness provides neither. |
| `161-regress-borders-restart.t` | 4 | unproven | The test requires an in-place restart. Its setup also uses i3's `border 1pixel` spelling, which sway rejects in favor of `border pixel 1` (`sway/commands/border.c:13-99`). |
| `162-regress-dock-urgent.t` | 4 | finished: 1 pass; 2 documented skip; 1 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `168-regress-fullscreen-restart.t` | 1 | unproven | Its sole liveness assertion follows a rejected `restart`; the harness cannot test state across an in-place compositor restart. |
| `188-regress-focus-restart.t` | 11 | unproven | All assertions pass around a rejected `restart`; the harness cannot test focus state across an in-place compositor restart. |
| `248-regress-urgency-clear.t` | 4 | unproven | Configuration translation stops before assertions, and the test requires an X11 `_NET_ACTIVE_WINDOW` client message. The native Wayland harness cannot generate that input. |
| `231-ipc-floating-event.t` | 2 subtests (6 inner assertions) | finished: 1 pass; 1 fail; 4 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `238-ipc-binding-event.t` | 13 | finished: 7 pass; 6 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `226-internal-workspaces.t` | 5 | 1 pass; 4 fail; classified 4 skip | Assertion 1 passes because the adapter excludes the synthetic `__i3` output while enumerating workspaces, as upstream i3test does (`tests/i3/lib/i3test.pm:311-317`); sway fabricates `__i3` and `__i3_scratch` only in GET_TREE (`sway/sway/ipc-json.c:459-499`). Assertions 2–5 fail because they enforce i3's reserved `__` prefix across workspace switch, move, rename, and startup binding discovery (`i3/src/commands.c:318,912,2116`; `i3/src/workspace.c:231`). Sway has no corresponding name guard: it creates arbitrary command targets (`sway/sway/commands/workspace.c:220-227`), creates arbitrary move targets (`sway/sway/commands/move.c:450-504`), rejects only reserved command words during rename (`sway/sway/commands/rename.c:72-82`), and accepts arbitrary binding workspace names (`sway/sway/tree/workspace.c:356-490`). Swayward follows sway, so all four failures are classified as i3-only policy. The former row incorrectly reported them as emitted TAP skips. |
| `267-regress-mark-restart.t` | 1 | unproven | Its sole liveness assertion follows a rejected `restart`; the harness cannot test marks across an in-place compositor restart. |
| `315-long-commands.t` | 4 | 3 pass; 1 fail; classified 4 skip | The unchanged run fails the x-position assertion and reports three accidental no-change passes because swayward rejects `move window container to window container to window container to left`. Sway also removes at most one optional `window|container` word and one optional `to`, then rejects the remaining `container ...` tokens (`sway/sway/commands/move.c:994-1024`). All four assertions depend on i3-only redundant-word parsing, so none proves sway-compatible movement behavior. The former `0 pass; 4 skip` result was a classification rather than the emitted TAP result. |
| `316-transient-for-loop.t` | 1 | unproven | The unchanged file stops before assertions because `popup_during_fullscreen smart` has no translated configuration surface. A temporary diagnostic omitting only that directive reaches the unavailable X11 `transient_for` operation. Sway follows X11 parent chains for this check (`sway/sway/desktop/xwayland.c:352-364`), while native xdg-shell parent assignment rejects ancestor cycles at protocol handling time (`wlroots/types/xdg_shell/wlr_xdg_toplevel.c:178-218`). The adapter cannot reproduce the malformed X11 loop without fabricating X11 state. |
| `310-client-message-sticky.t` | 6 | 1 reached and passed; no TAP plan; 5 unproven | The initial no-floating-window baseline passes, then the first raw `_NET_WM_DESKTOP` client message fails loud. The file emits no TAP plan. The remaining assertions require that X11 request, an EWMH dock window, and an X11 configure request; native `xdg_toplevel` cannot send those inputs. Command-driven sticky behavior is covered by `285-sticky.t`, and sway considers stickiness active only for floating containers (`sway/sway/tree/container.c:1705-1711`), but neither fact proves this X11 client-message regression. |

| `221-floating-type-hints.t` | 8 | finished: 0 pass; 8 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
All seven i3 files using `cmp_tree` avoid X11-only client operations and belong to the 217-file protocol-portable set. All are measurable with the current adapter: `294-focus-order.t`, `302-tree.t`, `306-move-to-parent.t`, `307-focus-next-prev.t`, `308-focus_wrapping.t`, `309-crash-move-parent.t`, and `550-split-redundant-containers.t`. The adapter builds compact `H`/`V`/`S`/`T` trees through real sway commands instead of i3-only `append_layout`, then ports upstream `verify_layout`'s token-by-token layout, name, and focused-node assertions without flattening or fabricating nodes. Floating layout strings remain fail-loud because sway omits i3's `floating_con` wrappers (`sway/sway/ipc-json.c:532-540`; `i3/src/ipc.c:619-633`). `Test::More::subtest` argument forwarding remains covered by the adapter's unprototyped wrapper.

| `269-focus-stack-above.t` | 5 | 1 reached and passed; no TAP plan; 4 unproven | The initial focus check passes, then the unchanged file calls X11 `ConfigureWindow` with `STACK_MODE_ABOVE`. The missing adapter method aborts the file before it emits a TAP plan. Sway handles configure requests only in its Xwayland path and does not translate stack mode into focus in its request-configure handler (`sway/desktop/xwayland.c:578-606`); native xdg-shell exposes no equivalent request. Assertions 2–5 remain unproven. |
| `302-tree.t` | 15 | finished: 12 pass; 3 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `304-ipc-workspace-init.t` | 9 | 6 pass; 3 unproven | Focus-only output changes, ordinary workspace creation/recreation, window moves, and back-and-forth init counts pass. The three failing subtests depend on startup `workspace X output fake-1` state surviving the adapter's fake-output replacement; reload creates configured workspaces before the old headless output is replaced, so `X` is materialized on `fake-0`. This lifecycle is not sway startup and cannot establish the expected output or deletion/recreation counts. Sway chooses configured output before creating a workspace and emits `init` immediately after adding it (`sway/tree/workspace.c:153-182,256-268`); event payload shape remains covered by native fixtures, while this ordering/count remains unpinned by a real sway capture. |
| `306-move-to-parent.t` | 2 | finished: 2 fail on i3-only comma criteria restart | Both unchanged command chains introduce `[con_mark=_a]` after a comma. Sway extracts a fresh criteria block only after `;`; after `,` it retains the current materialized target set and parses the next `[` as a command (`sway/sway/commands.c:232-255`). Swayward follows that scope rule, so the final criteria-targeted focus is rejected and each top-level comparison fails only its focus assertion; all shape assertions pass after the ancestor-insertion fix. A temporary diagnostic changing the two criteria separators to semicolons passes both strict comparisons, including 4/4 and 5/5 inner assertions. |
| `550-split-redundant-containers.t` | 8 | finished: 7 pass; 1 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `541-resize-set-tiling.t` | 32 | finished: 27 pass; 5 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `546-empty-bindcommand.t` | 1 | unproven; fail-loud config boundary | The empty `bindsym X` is correctly rejected rather than becoming an inert binding: sway's bind parser requires at least a trigger and command (`sway/commands/bind.c:389-396`), and both the translator and typed-config loading path have regression coverage for rejection. The unchanged file asserts compositor liveness after launching with this invalid config, but the in-process harness deliberately refuses incomplete translation instead of starting a separate compositor with an invalid configuration, so its assertion cannot run. |
| `247-config-line-continuation.t` | 2 reached; no TAP plan | 2 pass; 6 later assertions blocked | The unchanged run's escaped-string phase passes and proves a variable-expanded title criterion changes the mapped window's border. The second phase joins a deliberately invalid command across more than 4096 bytes; sway's reader joins non-comment lines before config parsing and variable expansion (`sway/config.c:665-690,732-812`), but swayward then eagerly rejects the translated KDL binding as `Unknown/invalid command '0001-This'` rather than launching for the liveness check. This is the documented [bound-command validation](../../docs/KNOWN_DEVIATIONS.md#bound-command-validation) difference. Continuing with a partial config would manufacture a pass, so the remaining six assertions are blocked and unproven. Native tests prove valid and invalid continued commands longer than 4096 bytes are preserved without truncation, continuation precedes variable expansion, and typed validation reports the complete offending command. |
| `201-config-parser.t` | 32 | skip all: i3-only parser implementation | Every assertion invokes i3's standalone `test.config_parser` binary and compares private `cfg_*` callback traces or generated-parser error text; none starts or queries a compositor. The binary is compiled directly from i3's generated `config_parser.c` with `TEST_PARSER` (`i3/meson.build:693-703`; `i3/src/config_parser.c:447-498`). Swayward has a different typed KDL parser and a separate behavior-level translator suite, so emulating these internal callbacks would fabricate i3 parser implementation rather than test observable sway behavior. |
| `509-workspace_layout.t` | 2 | finished: 1 pass; 1 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `522-rename-assigned-workspace.t` | 9 | finished: 4 pass; 5 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `524-move.t` | 38 | blocked before assertions; corrected-spelling diagnostic: 34 pass, 4 skip | The unchanged file uses `workspace_layout stacked`, which sway rejects; sway accepts only case-insensitive `default`, `stacking`, and `tabbed` (`sway/sway/commands/workspace_layout.c:5-20`; `sway/sway/sway.5.scd:92`). A temporary diagnostic changing only `stacked` to `stacking` reaches all assertions. Four i3-only top-level child-count checks fail because sway preserves singleton stacked wrappers after moves: move calls `container_reap_empty`, which removes only empty containers, while singleton removal uses `container_flatten`, whose only caller is `split none` (`sway/sway/tree/container.c:525-556`; `sway/sway/commands/move.c:225,410,612,722`; `sway/sway/commands/split.c:34-41`). A sway 1.11 capture with two headless outputs confirmed that moving both leaves from one stacked container produces two singleton stacked containers on the destination. See [Singleton layout containers after moves](../../docs/KNOWN_DEVIATIONS.md#singleton-layout-containers-after-moves). |
| `543-move-workspace-to-multiple-outputs.t` | 63 | 18 pass; 45 fail: deferred startup model and i3-only output cycles | Criteria-targeted workspace moves now use each matched window's workspace as sway does (`sway/sway/commands.c:183-200,301-326`; `sway/sway/commands/move.c:630-669`): the same-output command succeeds and the explicit move to fake-1 moves workspace 3. This removes the parser gap but also turns four former accidental passes red by executing the unsupported list semantics, changing the file from 20 pass/43 fail to 18 pass/45 fail. The initial workspace-3 checks separately observe the deferred startup-workspace defect: fake-2 starts on duplicate workspace `1` rather than sway's next free number (`sway/sway/tree/workspace.c:334-490`). The remaining cycles are i3-only: sway's workspace mover resolves only `argv[0]` as one name or geometric direction, so it neither recognizes `next` nor cycles through an output list (`sway/sway/commands/move.c:30-78,630-669`). See [Workspace output lists](../../docs/KNOWN_DEVIATIONS.md#workspace-output-lists). |
| `545-i3-registration.t` | 1 | skip: requires an in-process X11 window manager | The unchanged file reaches no assertions because it opens an X connection and queries ownership of the `WM_S0` selection. Sway creates an in-process wlroots XWM (`sway/sway/server.c:764-769`), whose registration window claims `WM_S0` (`wlroots/xwayland/xwm.c:2545-2585`). Swayward delegates the X11 window manager to `xwayland-satellite` and the Wayland harness has no X server; fabricating a selection owner would not test either architecture. |
| `290-keypress-numlock.t` | 22 outer assertions (62 TAP assertions) | 59 pass; 3 fail; classified 3 skip | Real injected keycode 77 updates Smithay's XKB state and leaves `ModifiersState::num_lock` latched after release. Binding lookup checks the NumLock-translated keysym before the raw keysym, matches explicit `Num`/`Mod2` modifiers exactly, and falls back to unqualified key bindings only when Caps/Num are the extra active modifiers, matching i3's generated lock variants (`i3/src/bindings.c:378-440,530-560`). Release ownership no longer emits extra events after another key completes the `Mod4+Return` chord. The two three-assertion `bindcode 133` checks have the modifier-event ordering mismatch described for `258-keypress-release.t`, not a keycode-offset mismatch. The top-level `--whole-window` option translates and both mouse assertions run; the binding passes without NumLock. The final outer assertion emits three failures because i3 converts a mouse symbol to a keycode before its common keycode branch generates NumLock and CapsLock variants (`i3/src/bindings.c:474-481,530-560`), while sway passes the keyboard's raw modifier set to mouse matching and requires exact equality (`sway/sway/input/seatop_default.c:120-143`). The three failures are classified as i3-only; adding a fallback to swayward would diverge from sway. The former row incorrectly reported them as TAP skips. |
| `313-include.t` | 8 reached; no TAP plan | 8 pass; remainder partly skip and partly unproven | Ordinary absolute, nested, and relative includes pass without manual-attention diagnostics. The unchanged file then aborts at command substitution, so later assertions are not TAP skips and are not freshly measured. Sway expands include arguments with `wordexp(3)`, including shell command substitution, after changing to the parent config's directory (`sway/config.c:594-625`). The static translator deliberately refuses to execute shell text from an untrusted config; that deliberate difference is a cited skip boundary. It supports variables, globs, parent-relative nested paths, and canonical-path duplicate prevention (`sway/config.c:555-591`). Later GET_VERSION included-file assertions are i3-only because sway's version reply has no included-file list, while GET_CONFIG returns only `config` (`sway/ipc-json.c:225-238`; `sway/ipc-server.c:908-917`). The remaining intervening include lifecycle assertions are unproven in the unchanged run. The previous `finished: 8 pass; 12 skip` claim overstated what the harness executes. |
| `511-scratchpad-configure-request.t` | 2 | finished: 0 pass; 2 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `284-ewmh-visible-name.t` | 5 | 5 unproven; native title-format substitute passes | The unchanged file reaches zero assertions because its first helper reads `_NET_WM_VISIBLE_NAME`, which native xdg-shell cannot expose. Native tests instead prove the portable behavior: `title_format` applies to a criteria-selected window, expands all eight sway placeholders into GET_TREE `name` and the rendered titlebar, updates both after a live client title change, and scans only the format string once. `%shell` is `xdg_shell`; `%class` and `%instance` are empty for native Wayland views, as in sway; and absent sandbox metadata is empty. Smithay exposes all three `wp_security_context_v1` values, which swayward now retains when accepting a restricted client. Sway stores the joined format on the selected container, reparses it when view metadata changes, and serializes the resulting `container->title` as GET_TREE `name` (`sway/sway/commands/title_format.c:9-35`; `sway/sway/tree/container.c:649-720`; `sway/sway/ipc-json.c:711-714`). |
| `551-net-wm-state-maximized.t` | 14 top-level checks (35 leaf assertions) | finished: 3 pass; 11 documented skip; 21 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |

| `554-commands-crash-for-window.t` | 101 subtests | 83 pass; 18 fail; all 101 liveness checks pass | The authoritative first unindented TAP plan is `1..101`; panic echoes are excluded. Each subtest first checks that the compositor remains responsive, then compares the command result with i3. The 101 liveness checks prove only crash resistance; they do not validate command semantics, and the outer result remains failed whenever the result assertion differs. Empty-workspace focus-layer, resize, and scratchpad commands now return sway's failures, removing ten command-result mismatches. The [per-subtest breakdown](#554-command-result-breakdown) records the 18 remaining mismatches and the resolved group. The matched `nop` remains sway-compatible (`sway/sway/commands/nop.c:3-5`). No failure is a harness limit. |
| `553-popup_during_fullscreen.t` | 19 | finished: 15 pass; 4 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `556-workspace-keeps-focus-after-move.t` | 3 reached; no TAP plan | 3 pass; 2 later assertions blocked by timeout | The unchanged run emits no rejected-command diagnostic. Initial focus, the first 1,000-event phase, and the first workspace move retain focus. The next 65,536 synchronous control round trips exceed the harness's 180-second file deadline, so assertions 4–5 after the intended X11 sequence-number overflow are blocked. Bisection of only that second loop measured 5.05s at 10, 5.31s at 100, 8.54s at 1,000, and 42.82s at 10,000: after fixed startup cost, about 3.8ms is added per round trip, predicting roughly 253s for 65,536. An in-process real-Wayland comparison measured 10, 100, 1,000, and 10,000 double round trips at 4.5ms, 47.7ms, 471ms, and 4.73s, also linear and about eight times faster. The bottleneck is the adapter's one Unix connection, JSON request/reply, and two Wayland round trips per `sync_with_i3`, not superlinear compositor work. Even a longer timeout would not reproduce X11's 16-bit event-sequence behavior, so the final two assertions remain an oracle limit. A timed full 95-file green manifest had no latent near-timeout file: the slowest were `274` at 15.31s and `132` at 13.84s. |

### 554 command-result breakdown

The categories below classify the 19 remaining failing result assertions and
ten resolved mismatches in `554-commands-crash-for-window.t`:

- **(a):** sway accepts the command, but swayward rejects it.
- **(b):** swayward accepts the command, but sway returns a failure in the same
  empty-workspace state.
- **(c):** i3 expects success, but both sway and swayward reject the command.
- **(d):** the harness cannot establish the result.
- **Sway-correct:** sway and swayward agree, but i3 expects a different result.
  These two cases fall outside A–D and are permanent sway-over-i3 decisions.

The remaining totals are (a)=0, (b)=0, (c)=16, (d)=0, and Sway-correct=2, which
accounts for all 18 remaining failures. The single category-(a) row and the ten
former category-(b) mismatches are retained below as resolved. Every subtest's
separate liveness check passes, but that proves only that the compositor did not
crash; it does not turn a mismatched command result into a pass.

| Subtest | Command | Category | i3 expected | swayward observed | Sway 1.11 observed and source |
| ---: | --- | --- | --- | --- | --- |
| 1 | `[all] kill` | (c) | success | failure: `No matching node.` | Same failure; empty criteria matches fail in `sway/sway/commands.c:301-304`. |
| 2 | `border 1pixel` | (c) | success | parse failure: unsupported border form | Failure: `Only views can have borders`; the empty-workspace check precedes border parsing in `sway/sway/commands/border.c:65-88`. The `1pixel` spelling is also absent from sway's accepted forms. |
| 3 | `border none` | (c) | success | failure: `Only views can have borders` | Same failure at `sway/sway/commands/border.c:65-67`. |
| 4 | `border normal` | (c) | success | failure: `Only views can have borders` | Same failure at `sway/sway/commands/border.c:65-67`. |
| 5 | `border pixel` | (c) | success | failure: `Only views can have borders` | Same failure at `sway/sway/commands/border.c:65-67`. |
| 6 | `border pixel 2` | (c) | success | failure: `Only views can have borders` | Same failure at `sway/sway/commands/border.c:65-67`. |
| 7 | `border toggle` | (c) | success | failure: `Only views can have borders` | Same failure at `sway/sway/commands/border.c:65-67`. |
| 11 | `focus child` | Sway-correct | failure | success, no focus change | Success with no child to select; `sway/sway/commands/focus.c:369-377` returns success unconditionally. |
| 13 | `focus floating` | resolved | failure | failure: `Failed to find a floating container in workspace.` | Same failure at `sway/sway/commands/focus.c:273-307,418-419`. |
| 15 | `focus mode_toggle` | resolved | failure | failure: `Failed to find a floating container in workspace.` | Same failure; the empty workspace selects the absent floating layer at `sway/sway/commands/focus.c:273-307,422-424`. |
| 18 | `focus parent` | Sway-correct | failure | success, no focus change | Success with no container to ascend from; `sway/sway/commands/focus.c:355-366` returns success unconditionally. |
| 22 | `focus tiling` | resolved | failure | failure: `Failed to find a tiling container in workspace.` | Same failure at `sway/sway/commands/focus.c:273-307,420-421`. |
| 69 | `resize grow height 10 px` | resolved | failure | failure: `Cannot resize nothing` | Same failure; sway checks for a current container before parsing the resize at `sway/sway/commands/resize.c:559-576`. |
| 70 | `resize grow width 10 px` | resolved | failure | failure: `Cannot resize nothing` | Same failure at `sway/sway/commands/resize.c:559-576`. |
| 71 | `resize grow width 10 px or 5 ppt` | resolved | failure | failure: `Cannot resize nothing` | Same failure at `sway/sway/commands/resize.c:559-576`. |
| 72 | `resize set 50 ppt 50 ppt` | resolved | failure | failure: `Cannot resize nothing` | Same failure at `sway/sway/commands/resize.c:559-570`. |
| 73 | `resize shrink height 10 px` | resolved | failure | failure: `Cannot resize nothing` | Same failure at `sway/sway/commands/resize.c:559-576`. |
| 74 | `resize shrink width 10 px` | resolved | failure | failure: `Cannot resize nothing` | Same failure at `sway/sway/commands/resize.c:559-576`. |
| 75 | `scratchpad show` | resolved | failure | failure: `Scratchpad is empty` | Same failure at `sway/sway/commands/scratchpad.c:100-105`. |
| 78 | `split t` | fixed | success | success | Fixed: `t` is now accepted as the toggle alias and the rejection text matches sway (`sway/sway/commands/split.c:54-82`). |
| 82 | `sticky disable` | (c) | success | failure: `No current container` | Same failure at `sway/sway/commands/sticky.c:20-23`. |
| 83 | `sticky enable` | (c) | success | failure: `No current container` | Same failure at `sway/sway/commands/sticky.c:20-23`. |
| 84 | `sticky toggle` | (c) | success | failure: `No current container` | Same failure at `sway/sway/commands/sticky.c:20-23`. |
| 88 | `title_format "%title"` | (c) | success | failure: `Only valid containers can have a title_format` | Same failure at `sway/sway/commands/title_format.c:14-17`. |
| 89 | `title_format "test: %title"` | (c) | success | failure: `Only valid containers can have a title_format` | Same failure at `sway/sway/commands/title_format.c:14-17`. |
| 90 | `title_window_icon off` | (c) | success | parse failure: unknown command | Same unknown-command failure; `title_window_icon` is absent from sway's complete general, config-only, and runtime command tables at `sway/sway/commands.c:41-143`. |
| 91 | `title_window_icon on` | (c) | success | parse failure: unknown command | Same unknown-command failure and complete-table absence as subtest 90. |
| 92 | `title_window_icon padding 3` | (c) | success | parse failure: unknown command | Same unknown-command failure and complete-table absence as subtest 90. |
| 93 | `title_window_icon toggle` | (c) | success | parse failure: unknown command | Same unknown-command failure and complete-table absence as subtest 90. |

The pinned oracle has 23 unvendored `regress` files after this batch. Five are dock-dependent (`150-regress-dock-restart.t`, `154-regress-multiple-dock.t`, `162-regress-dock-urgent.t`, `182-regress-focus-dock.t`, and `222-regress-dock-resize.t`), and six require an in-place compositor restart (`143-regress-floating-restart.t`, `150-regress-dock-restart.t`, `161-regress-borders-restart.t`, `168-regress-fullscreen-restart.t`, `188-regress-focus-restart.t`, and `267-regress-mark-restart.t`). Those classes are unproven by this Wayland harness. `150` belongs to both classes, leaving 13 other reachable candidates. The pinned checkout has `248-regress-urgency-clear.t`, not `248-regress-move-restart.t`.

## A resolved intermittent failure

The conformance runner used to fail one file per full-suite run and pass that
same file in isolation. Three causes were found, and all three are fixed:

- A 30-second per-file deadline. `132-move-workspace.t` runs 160 assertions and
  measures about 14s warm but 33s on a cold build cache, so the suite tripped it
  while the file was still emitting passing assertions. The deadline is now 180s.
- The harness leaked one control socket and one translated config per test. A
  temp directory had accumulated over 7000 of them, and Unix socket paths are
  capped near 108 bytes. Both are now removed through a `Drop` guard.

The third and last cause reported

    connect /run/user/1000/swayward-ipc.i3-tests.<pid>.<n>.sock: No such file or directory

from `tests/i3/lib/i3test.pm`, which opens a fresh connection per IPC request.
It was observed in `132-move-workspace.t`, `255-multiple-marks.t`, and a captured
`550-split-redundant-containers.t` failure. The capture proved that the socket
pathname disappeared while the Perl child was still running; `IpcServer::drop`
ran only after the failure. A full suite under `ulimit -n 64` failed explicitly
with `EMFILE` in unrelated fixture setup rather than this `ENOENT`, ruling out
accepted-stream descriptor leakage as the cause. `/run/user/1000` had 3.2 GB and
more than 800,000 free inodes, and the listen backlog was 4096.

Conformance IPC sockets now live inside a unique private directory named by the
test process and a process-global counter, rather than as top-level
`/run/user/$UID/swayward-ipc.*.sock` files. This isolates them from the mandatory
cleanup glob used by nested-compositor test scripts. The runner waits for the
Perl child before dropping the server, then removes the private directory. After
the change, 20 consecutive full `cargo test --all -- --nocapture` runs passed;
the captured failure had appeared on attempt 2 before the change. There is no
retry, sleep, or catch-and-continue path.

| `159-socketpaths.t` | 0 reached; skip-all | The runner skips the unchanged file because it requires a separate compositor process. The former `7 reached: 3 pass; 4 fail` result is stale. The file expects i3's `/tmp/i3-$user/ipc-socket.$pid`, `$XDG_RUNTIME_DIR/i3`, and `ipc-socket` directive. Sway instead defaults to `$XDG_RUNTIME_DIR/sway-ipc.<uid>.<pid>.sock`, falls back to `/tmp`, and adopts a preset `SWAYSOCK` only when that path does not exist (`sway/sway/ipc-server.c:99-146`). Swayward uses `swayward-ipc.<wayland-display>.<pid>.<counter>.sock`; the filename difference is harmless because clients discover it through `SWAYSOCK`. Native lifecycle tests verify safe `SWAYSOCK` adoption without replacing a live socket, stale-socket removal, cleanup, rejection of an overlong Unix path, and advertisement as both `SWAYSOCK` and `I3SOCK`. A capped nested process confirmed both exported variables and SIGTERM cleanup. A second capped process after SIGKILL preserved the old socket and selected a fresh default path, matching sway's anti-hijack condition. The in-process runner cannot vary the compositor's startup environment, so the remaining lifecycle assertions are unproven there. |
| `540-sigterm-cleanup.t` | 2 | file-level skip; 0 reached | The runner skips the whole file because it cannot send SIGTERM to its in-process compositor without destroying the test. The unchanged file emits `1..0 # SKIP`, so neither source assertion runs. Native lifecycle coverage verifies that dropping the real `IpcServer` removes its socket and that a second server can bind the same path after both stale and clean shutdown. The product event loop handles SIGTERM by stopping, which drops the server (`src/utils/signals.rs:46-56`; `src/ipc/server.rs:158-163`); sway registers SIGTERM with `term_signal` and removes its IPC socket when the display is destroyed (`sway/sway/server.c:158`; `sway/sway/ipc-server.c:99-116`). The previous row incorrectly credited one pass. |
## Current classification

The suite records 2,330 passes and 780 documented assertion skips. Each skip
carries an assertion number, a reason, and a `sway` or `i3` citation. Nine files
instead emit a cited file-level `1..0 # SKIP` plan; their 56 static source
assertions are not counted as executed assertions. The fresh real-TAP measurement
also exposes 14 failures and 44 unreached assertions. Those 58
assertions are backlog, not skips.

Most documented skips are structural: X11-only behavior, i3's own binary,
i3-only tree nodes, and bar protocol. The rest are places where sway
deliberately behaves differently from i3, and swayward follows sway.

## Unvendored-file audit and green ceiling

This audit opened all 66 files that were unvendored before
`511-scratchpad-configure-request.t` was added. Concurrent work has since
vendored 18 of them. With `301-shape.t` now measured, the remaining 48 divide
into 23 unreachable files, 20 i3-only subsystem files, and five files with
measured partial coverage. No unvendored file remains classified as a reachable
green candidate.

The **current green ceiling is 112 files**: the 108 green files derived from
`coverage.toml` plus
4 vendored files whose only obstacles are implementation or adapter gaps.
Swayward is 4 files below that ceiling.

Four files reached green during the coverage sweep and left this table:
`184-regress-float-split-resize.t`, `154-regress-multiple-dock.t`,
`182-regress-focus-dock.t` and `222-regress-dock-resize.t`. The three dock files
are no longer green: their checks never create the X11 dock premise. The ceiling
therefore excludes them along with other unavailable-input files. The ceiling
holds the documented
oracle limits fixed. It excludes every file with an i3-only assertion or an
input that the harness cannot ask, and does not promote a partial pass to green.

These 4 files define the gap-only set:

| Gap-only file | Remaining gap |
| --- | --- |
| `176-workspace-baf.t` | A preserving in-place compositor restart, which the in-process fixture cannot perform. |
| `297-assign-workspace-to-output.t` | Later sections need multi-output assignment lists; the file declares no plan and aborts after 9 assertions. |
| `503-workspace.t` | Per-output initial names and stored sway workspace order. |
| `522-rename-assigned-workspace.t` | Ordered output-assignment metadata separated from persistent workspace creation. |

The shared workspace gaps are specified in
`docs/specs/2026-09-16-workspace-order-model.md`. Each file's coverage row
records its source citations and any narrower prerequisite.

`528-workspace-next-prev-reversed.t` and `535-workspace-next-prev.t` were in
this table until their `8:*` assertions were measured against live sway 1.11 and
found to be i3-only. Sway parses every `8:*` name as number 8 and global
navigation never visits equal prefixes, so closing the workspace gap would not
make either file green. They moved out, and the ceiling fell from 107 to 105.
A score going down can be progress.

Mixed files do not belong in this table. For example, `166-assign.t` has an
eager-workspace failure, but it also has i3-only relative and primary output
assertions plus unavailable X11 dock assertions. Likewise,
`543-move-workspace-to-multiple-outputs.t` has a startup-workspace gap and
i3-only output-list cycling. Fixing the swayward gap would not make either file
green.

The categories use these source-backed boundaries:

- **X11 state or request unavailable:** sway handles X11 configure, property,
  client-message, and window-manager state through its in-process Xwayland view
  (`sway/sway/desktop/xwayland.c:578-622,816-869`). Swayward delegates the XWM
  to `xwayland-satellite`, which exposes ordinary `xdg_toplevel` surfaces. The
  harness therefore cannot address an X window, the X root window, or an X
  property. `xdg_surface.set_window_geometry` describes surface-local content;
  `xdg_toplevel.move` and `resize` are serial-gated interactive requests, not
  absolute configure requests (`smithay/src/wayland/shell/xdg/mod.rs:1048-1122`).
- **Restart or process lifecycle unavailable:** the runner hosts one compositor
  in process. It cannot replace that process while retaining clients, inspect
  its private startup paths, or observe shutdown cleanup. Sway exposes `reload`
  and `exit` but no runtime `restart` command in its complete command tables
  (`sway/sway/commands.c:44-144`; `sway/sway/sway.5.scd:102-415`).
- **Layout restoration unavailable:** sway has no `append_layout` command in
  those same complete command tables and runtime command reference.
- **i3 utility unavailable:** these files execute i3's own parser, migration,
  logging, dmenu, RandR-injection, or i3bar binaries rather than a compositor
  protocol. Sway's complete command tables do not provide these i3 programs
  (`sway/sway/commands.c:44-144`; `sway/sway/sway.5.scd:102-415`).

| Remaining file | Audit category | Mechanism and source |
| --- | --- | --- |
| `000-load-deps.t` | i3-only subsystem | Loads the Perl and XCB dependencies of i3's own test runner; it does not query a compositor (`i3/testcases/t/000-load-deps.t:5-19`). |
| `002-i3-sync.t` | i3-only subsystem | Asserts i3's private `I3_SYNC` X ClientMessage handshake (`i3/testcases/lib/i3test.pm.in:728-787`; `i3/src/handlers.c:813-815`). The local helper is only a harness barrier, not that protocol. |
| `004-unmanaged.t` | unreachable | Requires an X11 override-redirect window and compares its pre-map and post-map X rectangles. Native `xdg_toplevel` has neither override-redirect state nor a client-selected absolute rectangle; see the X11 boundary above. |
| `114-client-leader.t` | unreachable | Uses `WM_CLIENT_LEADER`, `WM_TRANSIENT_FOR`, X input focus, and `window_properties.transient_for`. Native xdg-shell can express only the parent relation, not the leader or X IPC fields (`sway/sway/desktop/xdg_shell.c:228-235`; `sway/sway/ipc-json.c:670-700`). |
| `158-wm_take_focus.t` | unreachable | Receives and distinguishes the ICCCM `WM_TAKE_FOCUS` ClientMessage and reads `_NET_WM_STATE_FOCUSED`; both require the in-process XWM and an addressable X window (`sway/sway/desktop/xwayland.c:816-869`). |
| `163-wm-state.t` | unreachable | Reads ICCCM `WM_STATE_NORMAL` and `WM_STATE_WITHDRAWN` from an X window. No corresponding property exists on native xdg-shell surfaces; see the X11 boundary above. |
| `171-config-migrate.t` | i3-only subsystem | Executes `i3-migrate-config-to-v4 --v3` and checks its private v3 rewrite rules. Sway has no such utility in its command surface (`sway/sway/commands.c:44-144`). |
| `175-startup-notification.t` | unreachable | The first placement scenario has a Wayland analogue: an activation token used before map assigns the view to the launch workspace (`sway/sway/xdg_activation_v1.c:20-41`), and `exec` creates tokens unless `--no-startup-id` is set (`sway/sway/commands/exec_always.c:28-67`). Later assertions instead propagate `_NET_STARTUP_ID` through `WM_CLIENT_LEADER`; xdg activation tokens have no leader-property inheritance. The complete unchanged file therefore cannot become green without fabricating X11 metadata. |
| `195-net-active-window.t` | unreachable | Drives `_NET_ACTIVE_WINDOW`, reads the root property, and addresses X dock and scratchpad windows by XID. xdg-activation can substitute only the activation request, not the root property or X identities (`sway/sway/xdg_activation_v1.c:8-49`). |
| `207-shmlog.t` | i3-only subsystem | Executes `i3-dump-log` and the i3-only `shmlog` command. Sway's complete command tables contain neither (`sway/sway/commands.c:44-144`). |
| `209-ewmh-net-workarea.t` | unreachable | Creates, reads, and expects deletion of `_NET_WORKAREA` on the X root window. The satellite boundary does not expose the X root to the compositor; see the X11 boundary above. |
| `214-layout-restore-criteria.t` | i3-only subsystem | Every behavior under test starts with `append_layout` and inspects JSON swallow placeholders. Sway has no layout-restoration command; see the layout-restoration boundary above. |
| `215-layout-restore-crash.t` | i3-only subsystem | Exercises malformed and partial `append_layout` JSON plus placeholder liveness. Sway has no `append_layout`; see the layout-restoration boundary above. |
| `216-layout-restore-split-swallows.t` | i3-only subsystem | Asserts `append_layout`'s private JSON `swallows` representation. Sway has no `append_layout`; see the layout-restoration boundary above. |
| `217-NET_CURRENT_DESKTOP.t` | unreachable | Reads and sends `_NET_CURRENT_DESKTOP` on the X root window. Workspace IPC is portable, but it cannot prove this EWMH property or ClientMessage path; see the X11 boundary above. |
| `223-net-client-list.t` | unreachable | Every assertion reads `_NET_CLIENT_LIST`, an array of X window IDs on the X root, including dock exclusion. Neither the property nor those XIDs cross the satellite boundary. |
| `229-cleanup-tmpdir.t` | i3-only subsystem | Inspects i3's private temporary directory and config-path X atom across exit and in-place restart. The in-process harness exposes neither process lifecycle; see the lifecycle boundary above. |
| `230-floating-fullscreen-restart.t` | i3-only subsystem | All postcondition assertions depend on preserving a floating X window across compositor restart. The harness cannot restart while retaining clients, and sway has no runtime restart command; see the lifecycle boundary above. |
| `234-ewmh-desktop-names.t` | unreachable | Reads `_NET_DESKTOP_NAMES` from the X root. `GET_WORKSPACES` can test workspace names, but substituting it would not test this EWMH publication path. |
| `239-net-close-window-request.t` | unreachable | Sends `_NET_CLOSE_WINDOW` to the X root. `xdg_toplevel.close` runs in the opposite direction, compositor to client, so no native client request can substitute for this EWMH message (`sway/sway/desktop/xdg_shell.c:252-256`). |
| `249-layout-restore-floating.t` | i3-only subsystem | Loads and kills an i3 JSON `floating_con` placeholder through `append_layout`. Sway has neither that command nor i3's floating wrapper; see the layout-restoration boundary above. |
| `250-layout-restore-multiple-criteria.t` | i3-only subsystem | Tests selection among multiple JSON swallow criteria loaded by `append_layout`. Sway has no layout-restoration command. |
| `253-multiple-net-wm-state-atoms.t` | unreachable | Reads `_NET_WM_STATE_STICKY` and `_NET_WM_STATE_FULLSCREEN` atoms directly from X windows. Command state is covered elsewhere, but it cannot prove X property publication through the satellite. |
| `259-net-wm-user-time.t` | unreachable | Sets `_NET_WM_USER_TIME` before map and tests X focus prevention. Native xdg-shell has no user-time property; sway receives this metadata through its Xwayland view (`sway/sway/desktop/xwayland.c:816-869`). |
| `278-layout-restore-output.t` | i3-only subsystem | Creates workspace objects and swallow placeholders by loading i3 JSON through `append_layout`. Sway has no layout-restoration command. |
| `283-net-wm-state-hidden.t` | unreachable | Every assertion reads `_NET_WM_STATE_HIDDEN` from X windows. xdg-toplevel's optional `suspended` configure state is not this asserted X property and sway's XWM owns the atom path. |
| `288-i3-floating-window-atom.t` | unreachable | Reads i3's proprietary `I3_FLOATING_WINDOW` X property. Sway does not publish an i3-private atom; portable floating state is already exposed through GET_TREE. |
| `294-update-ewmh-atoms.t` | unreachable | Reads three X-root EWMH properties after workspace deletion and rename. Workspace IPC cannot prove root-property publication; see the X11 boundary above. |
| `300-restart-non-utf8.t` | i3-only subsystem | The intended assertion follows an in-place restart with a live client. The harness cannot preserve that client through compositor replacement; see the lifecycle boundary above. |
| `305-restart-reply.t` | i3-only subsystem | Its sole assertion requires the i3 runtime `restart` command. Sway's complete runtime command table has no restart entry (`sway/sway/commands.c:112-144`). |
| `314-window-icon-padding.t` | i3-only subsystem | Uses i3's `open` and `title_window_icon` commands and asserts the i3-only `window_icon_padding` IPC field. Neither command exists in sway's complete command tables (`sway/sway/commands.c:44-144`). |
| `318-i3-dmenu-desktop.t` | i3-only subsystem | Executes `i3-dmenu-desktop` against generated `.desktop` files and a fake `i3-msg`; no compositor starts. Sway's command surface does not provide this i3 utility. |
| `323-net-frame-extents.t` | unreachable | Every assertion reads `_NET_FRAME_EXTENTS` from an X window. Border behavior is portable and covered elsewhere, but it cannot establish this XWM-owned property. |
| `521-ewmh-desktop-viewport.t` | unreachable | Reads `_NET_DESKTOP_VIEWPORT` coordinate pairs from the X root. Output and workspace IPC cannot prove this EWMH publication path. |
| `525-i3bar-mouse-bindings.t` | i3-only subsystem | Launches a real i3bar X window, sends XTEST buttons to it, and checks i3bar's IPC forwarding. Sway does not ship i3bar; external bars use IPC and their own input handling. |
| `529-net-wm-desktop.t` | unreachable | Every phase reads, sets, or sends `_NET_WM_DESKTOP` for an XID, including the sticky sentinel. Workspace commands cover related state but cannot prove this XWM-owned property and ClientMessage path. |
| `532-xresources.t` | unreachable | Populates the X root `RESOURCE_MANAGER` property and uses i3's `set_from_resource`. Sway has no such directive in its complete config command table (`sway/sway/commands.c:44-110`). |
| `533-randr15.t` | i3-only subsystem | Injects binary RandR replies into i3's test server and combines them with i3bar output canonicalization. This does not drive sway's output-management protocol or the headless output control. |
| `536-net-wm-desktop_mm.t` | unreachable | Reads `_NET_WM_DESKTOP` after a cross-output move. The move is portable, but the sole assertion is the X property value; see the X11 boundary above. |
| `542-layout-restore-remanage.t` | i3-only subsystem | Changes X properties after loading JSON swallow placeholders through `append_layout`. Both the restoration engine and X remanage path are outside the native harness. |
| `548-motif-hints.t` | unreachable | Writes `_MOTIF_WM_HINTS` before and after map and inspects resulting borders. Native xdg-decoration negotiation is compositor/client protocol, not a mutable Motif property (`sway/sway/desktop/xwayland.c:816-869`). |
| `552-net-wm-state-multiple-changes.t` | unreachable | Sends one X ClientMessage containing fullscreen and sticky atoms. Native xdg-toplevel can request fullscreen but has no sticky state or multi-atom request, so no equivalent request preserves the tested atomic input. |
| `555-i3bar-workspace-output-assignment.t` | i3-only subsystem | Launches and terminates the i3bar binary, then parses its private verbose drawing log. Sway does not ship i3bar, and compositor workspace IPC cannot prove i3bar rendering. |

The initial X11-protocol exclusions are `113-urgent.t`,
`162-regress-dock-urgent.t`, `196-randr-output-names.t`,
`209-ewmh-net-workarea.t`, `234-ewmh-desktop-names.t`,
`277-ipc-window-urgent.t`, `294-update-ewmh-atoms.t`,
`521-ewmh-desktop-viewport.t`, and `533-randr15.t`.

Tests that inspect X11, EWMH, RandR, XKB, or raw X events are outside this
adapter's scope. Tests requiring unsupported i3 commands such as
`append_layout` are added only when swayward implements the corresponding sway
command.
