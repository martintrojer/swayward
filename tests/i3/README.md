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
`tests/i3/coverage.toml` records each assertion currently classified for this
limit. Run `contrib/coverage-report` for the current census. Files that mention
a wrapper but cannot reach or isolate the assertion because of X11 input,
pointer input, layout restoration, or floating-split behavior have their own
classifications.

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

The local `ExtUtils::PkgConfig` adapter implements only `atleast_version`. It
reports `xcb-xkb` as available because the harness implements the tested XKB
group changes in-process and never links that X11 library. Other packages use
the host's real `pkg-config --atleast-version`. Every other
`ExtUtils::PkgConfig` method dies loudly; the adapter does not provide package
flags or metadata.

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

An audit at commit `current` found 28 textual references to
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
| Cited skip | `159`/`196`/`235`/`262`/`540` skip dispatch and reason selection; assertion skips for `193`, `260`, `294`, `287`, `228`, `518`, `194`, `541`, `319`, `133`, `132`, `166`, `551`, `164`, and `285` | 17 | Each branch marks an assertion or file that cannot hold against sway or the Wayland harness. Each file remains outside the derived green set; its coverage row gives the source citation and boundary. The five-file skip-all dispatch and its two-way reason selection count as two references. |
| Setup or lifecycle adaptation | `298` removes i3's unsupported `ipc_kill_timeout 500` test directive; `202` finds sway's nested scratchpad workspace; `289` preserves shutdown subscriptions and requests in-process exit; `553` maps transient parents and isolates i3's unsupported `all` policy | 4 | The row records sway's fixed 4 MB write-buffer behavior and the native backpressure test. |
| Weakens the oracle | `201`; both `238` checks; `509`; `307`; `510`; `231`; `245`; `320`; `513`; `120`; `169`; `512`; `550-split-redundant-containers`; `302`; and both `527` checks | 17 | Fifteen files containing 78 TAP skips were listed as passing in full. This masked the cited incompatibilities as false greens. The audit removed all fifteen from the old `passing.txt` manifest and deleted the duplicate, unreachable `509` branch. |

The category-(d) branches did not conceal an unknown compositor regression: the
file rows already recorded each skip and its sway source. They weakened the
old manifest oracle by letting a zero exit status with TAP skips count as a full
pass.

The default gate derives its green set from `tests/i3/coverage.toml` through
`passing_tests()`. A file is green only when its row has a captured, nonzero TAP
plan and records every assertion as passing. This keeps the green set and the
coverage census in one source of truth.

The runner enforces the no-skips rule because the old manifest did not. The
audit found fifteen listed files emitting seventy-eight cited TAP skips, which
the manifest reported as fully green. The coverage rows below had recorded
those skips, so no product regression was hidden. At that point, removing the
entries took the green count from 108 to 93. A derived green file that emits a
skip now fails the runner.

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
its TAP output** (`contrib/tap-count`), not by reading prose. All 26 measured
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

In four the command is load-bearing, so their assertions stay unproven rather
than skipped -- the commands execute, but the state they were meant to set up
never exists:
`127-regress-floating-parent.t`, `142-regress-move-floating.t`,
`147-regress-floatingmove.t` and `151-regress-float-size.t`. In the other three
it is incidental to what the file checks, and they pass in full:
`126-regress-close.t`, `144-regress-floating-resize.t` and
`152-regress-level-up.t`.

`148-regress-floatingmovews.t` also uses the obsolete command, but its residual
focus assertion is valid for both tiled and floating containers and now passes.
Sway branches while reparenting the two container types, then restores focus
through the same path (`sway/sway/commands/move.c:204-232,598-608`).

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
| `528-workspace-next-prev-reversed.t` | 38 | 16 pass; 22 fail: mixed deferred-model and i3-only oracle | All assertions run without rejected commands. The initial numeric failures expose swayward's deferred creation and ordering model. The later `8:*` expectations are i3-only: sway parses every `8:*` name as number 8, and global next/prev select only strictly greater or smaller numbers, so they do not visit equal-prefix workspaces consecutively. Sway scans stored per-output lists only to break ties and to order nonnumeric names (`sway/sway/tree/workspace.c:540-677`). See the workspace index and placeholder site mapping model. |
| `535-workspace-next-prev.t` | 38 | 16 pass; 22 fail: mixed deferred-model and i3-only oracle | This complementary output order has the same split as `528`. The initial numeric failures depend on the deferred startup and ordering model. The later expectation that global navigation visits all equal-prefix `8:*` workspaces is i3-only. Sway uses strict numeric comparisons for global next/prev and stored-list order only for equal-choice tie breaking and nonnumeric traversal (`sway/sway/tree/workspace.c:540-677`). All assertions run without rejected commands. |
| `297-assign-workspace-to-output.t` | Later sections need multi-output assignment lists; the file declares no plan and aborts after 9. |
| `503-workspace.t` | 18 | 2 pass; 16 fail: deferred workspace model | Two real fake outputs exist and no command is rejected. The first failure proves the second output starts on `1` instead of sway's lazily derived next free workspace `2`; all global and per-output navigation checks then observe swayward's unsorted creation order. Sway derives one initial name per output from assignments, then bindings, then the first free number and sorts each output at insertion (`sway/sway/tree/workspace.c:334-490,255-259`). This is the documented deferred workspace identity/lifecycle model, not a parser defect. |
| `518-interpret-workspace-numbers.t` | 4 | finished: 3 pass; 1 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `320-mouse-bindings.t` | 13 | finished: 11 pass; 2 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `316-drag-container.t` | 58 inner assertions across 9 subtests | finished: 9 pass; 8 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `534-dont-warp.t` | 2 | finished: 0 pass; 2 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `512-move-wraps.t` | 10 | finished: 10 pass | Fresh real-TAP measurement after matching sway's directional output wrapping. |
| `245-move-position-mouse.t` | 8 | finished: 6 pass; 2 documented skip | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `504-move-workspace-to-output.t` | 31 | 10 pass; 21 fail; direct-node diagnostic: 24 pass, 7 fail | The adapter removes the exact test-only block that disables i3bar, then creates the file's two real fake outputs. The unchanged file reaches every assertion. Twenty-one checks use i3's output `content` wrapper, which sway omits. A direct-workspace diagnostic passes 24 assertions and leaves seven deferred-model or hierarchy failures: the second output starts on `1` instead of `2`, moving the last source workspace creates `1` instead of i3's `3`, and dependent membership checks inherit those identities. Named, directional, optional-`output`, and criteria-targeted workspace moves are covered natively, including adjacent-only direction lookup from the moved workspace's output, failure at an output edge, matched-window workspace context, and preserved floating-window coordinates. The unchanged file still expects i3's directional wrapping, while sway returns failure when no adjacent output exists (`sway/sway/commands/move.c:630-665`; `sway/sway/tree/workspace.c:436-490`). |
| `266-net-moveresize-window.t` | 12 | 0 reached; no TAP plan; 12 classified as i3-only | The unchanged file stops before assertions when its first raw X11 client message fails loud. i3 implements `_NET_MOVERESIZE_WINDOW` by converting its position and size flags into an X11 configure request (`i3/src/handlers.c:960-988`) and advertises the atom (`i3/include/i3-atoms_NET_SUPPORTED.xmacro.h:39`). A complete search finds no atom handler or mention in sway's source or command reference, so sway does not implement this i3 behavior even for its in-process Xwayland clients. Swayward's `xwayland-satellite` owns X11 window-manager messages and presents ordinary `xdg_toplevel` surfaces; that boundary cannot carry this absolute request to swayward. As documented for `112-floating-resize.t`, `xdg_surface.set_window_geometry` is surface-local, while `xdg_toplevel.move` and `resize` are serial-gated interactive requests without target geometry. All twelve unexecuted assertions are therefore classified as i3-only rather than treated as unproven sway behavior; the unchanged file emits no TAP skips. The fail-loud `send_event` stub remains unchanged. |
| `291-swap.t` | 11 reached; no TAP plan | 11 pass; remainder unproven | The fresh unchanged run reaches one liveness check, one cross-workspace `con_id` result, and nine same-workspace mark-swap shape/focus assertions. The initial `swap container with con_id 1` is rejected as expected; its `does_i3_live` pass proves only that the invalid request did not crash. Map-time `mark` rules translate through the existing `sway-for-window-command` path and assign globally unique marks (`sway/sway/commands/mark.c:10-61`). The file then aborts at line 201 because the native client has no X11 `fullscreen` method. Later fullscreen permutations, X11 `id` targets, and floating rectangles remain unproven: sway's `id` swap branch exists only with Xwayland, while `con_id` and mark are portable (`sway/sway/commands/swap.c:13-93`). The old row undercounted the fresh passes by one. |
| `289-ipc-shutdown-event.t` | 4 | finished: 0 pass; 4 unreached | Fresh real-TAP measurement after removing assertion interception. `coverage.toml` records each retained skip with its assertion number, reason, and sway citation. |
| `271-for_window_tilingfloating.t` | 20 | 0 reached; no TAP plan; 12 classified as i3-only, 8 later assertions blocked | Of the six criteria, two are sway-supported current-state predicates: `tiling` and `floating` (`sway/sway/criteria.c:607-611`; `sway/sway/sway.5.scd:1044-1045,1068-1069`). The translator maps them exactly to KDL `is-floating=false|true`, and now preserves all valid `mark` flag combinations through the existing map-time command surface. Four are i3-only provenance predicates: `tiling_from=auto|user` and `floating_from=auto|user` (`i3/src/match.c:417-443`). A complete search of sway source and runtime documentation finds no provenance criterion, so those forms remain fail-loud and their 12 dependent assertions are classified as i3-only rather than conflating “was tiled or floating by policy” with current state. The unchanged file emits no TAP or rejected-command diagnostic because translation stops before launch. The eight current-state assertions remain unproven because this adapter's `open_floating_window` maps a native toplevel tiled and changes it afterward, unlike upstream's pre-map X11 utility type; native tests prove tiled-state map-time marks and global mark uniqueness. |

## Coverage table

There isn't one here. [`coverage.toml`](coverage.toml) is the source of
truth, one entry per file with its assertion counts and an itemised,
cited table per skip. A prose copy of that data went stale the moment a
file moved, and the copy this section used to hold had drifted to 183 of
242 files.

Render whichever view you need:

```sh
contrib/coverage-report           # the census
contrib/coverage-report --gaps    # the work queue, largest backlog first
contrib/coverage-report --json    # machine-readable
contrib/coverage-report --check   # fails on an undocumented skip
```

To read why one file behaves as it does, open its `[files."NNN-name.t"]`
entry in `coverage.toml`. The reason and the sway citation live there,
next to the numbers they explain.

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

The suite records 2,305 passes and 780 documented assertion skips. Each skip
carries an assertion number, a reason, and a `sway` or `i3` citation. Nine files
instead emit a cited file-level `1..0 # SKIP` plan; their 56 static source
assertions are not counted as executed assertions. The fresh real-TAP measurement
also exposes 30 failures and 56 unreached assertions. Those 86
assertions are backlog, not skips.

Most documented skips are structural: X11-only behavior, i3's own binary,
i3-only tree nodes, and bar protocol. The rest are places where sway
deliberately behaves differently from i3, and swayward follows sway.

## What we have not vendored

242 of i3's 285 files are vendored. The other 43 are listed in
[`unvendored.toml`](unvendored.toml), each with a category and a reason:

- **unreachable** — the premise cannot exist here. X11 protocol behaviour,
  an in-place compositor restart, i3's own binaries. Vendoring one would add
  a file that can never pass.
- **i3_only** — an i3 subsystem sway does not have.

`contrib/coverage-report --unvendored` checks that list against the tree, so
it cannot claim a file we have already vendored or omit one we have not. The
prose audit this replaced had no such check and had drifted by five files
before anyone noticed.

The **current green ceiling is 116 files**: the 108 green in `coverage.toml` plus
8 vendored files whose only obstacles are implementation or adapter gaps.
A file carrying a documented skip can never join them, because a documented
skip records that the assertion is wrong about sway and is permanent; a file
with no captured TAP plan cannot be green either. `contrib/coverage-report`
derives the figure as `green_ceiling`, so it cannot be retyped stale.

The ceiling is arithmetic, not a forecast. Most of these 8 are blocked on an
X11 premise or an in-place restart, so the likely outcome for several is a
documented skip rather than a pass, which would lower the ceiling.

These 8 files define the gap-only set:

| Gap-only file | Reached | Remaining gap |
| --- | --- | --- |
| `182-regress-focus-dock.t` | 0/1 | An X11 dock window type the adapter fails loud on before any assertion runs. |
| `211-regress-urgency-assign.t` | 1/3 | EWMH urgency hints set through raw X11 properties. |
| `231-ipc-floating-event.t` | 1/6 | i3's floating IPC event semantics, which differ from sway's. |
| `289-ipc-shutdown-event.t` | 0/4 | A preserving in-place compositor restart, which the in-process fixture cannot perform. |
| `511-scratchpad-configure-request.t` | 0/2 | An X11 ConfigureRequest choosing an absolute desktop rectangle. |
| `527-focus-fallback.t` | 1/2 | EWMH focus state read through X11 properties. |
| `534-dont-warp.t` | 0/2 | An X11 ConfigureRequest choosing an absolute desktop rectangle. |
| `553-popup_during_fullscreen.t` | 15/19 | i3-only popup-during-fullscreen placement semantics. |
