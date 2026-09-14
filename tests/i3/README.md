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

The adapter gives each `open_window` object the real swayward container id and
translates a test-side `node.window` read for a native `xdg_shell` node to that
same `node.id`. Its X11 `mapped` compatibility check reads workspace visibility
from `GET_WORKSPACES`; sway's `GET_TREE` `focused` flag identifies only the
single default-seat focus node, not every visible workspace. It does not add `window` to the IPC object: key iteration and
`exists` still expose sway's native Wayland schema, which omits `window`
(`sway/sway/ipc-json.c:670-683`). These comparisons prove container identity and
the behavior addressed through it, but do not prove X11 window identity or
Xwayland serialization.

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

For `240-focus-on-window-activation.t` only, the adapter recognizes the test's
`_NET_ACTIVE_WINDOW` message shape and substitutes a real xdg-activation token
request plus activation request for the same native Wayland surface. Other X11
events still die loudly. The token deliberately has no input serial, matching
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
IPC serializer preserves that stored order. Swayward instead inherits niri's
trailing unnamed workspace as an internal creation target. This makes
"sway-visible" context-dependent: IPC, relative navigation, and cleanup need
different membership rules. One shared predicate broke `117-workspace.t`
because inactive transient named workspaces must disappear from IPC, while
configured empty workspaces and active empty workspaces remain visible.

Sorting also requires materializing an occupied implicit workspace's
index-derived number as stable identity. That conflicts with niri's current
name and persistence fields, which cleanup uses to decide whether an empty
workspace survives. Sorting only during serialization is not equivalent because
sway sorts during insertion and navigation observes that order. This is a
deferred design issue, not a closed compatibility decision. A correct fix needs
a workspace identity and lifecycle representation distinct from niri's name and
persistence state, with coordinated changes to creation, cleanup, navigation,
IPC, and output assignment. In `117-workspace.t`, assertions 11 and 15 exercise
next/previous order but cannot establish sway's insertion-sorted model;
assertions 33 and 41 only prove that no internal placeholder leaks as workspace
`4` or `7`. The file's reported 89 passes and 3 unrelated skips are unchanged.

The adapter cannot reproduce a compositor restart. Rebuilding `Fixture` destroys
its Wayland clients and windows, while `State::reload_config` preserves them and
is not a restart. `launch_with_config` is supported only for independent config
phases: it reloads the requested config, and `exit_gracefully` removes all test
windows before the next phase. Tests that depend on state surviving a restart
remain unproven. In particular, `176-workspace-baf.t` launches two compositor
configurations and later verifies back-and-forth state across `restart`; using
reload for either transition would test a weaker lifecycle.

The adapter cannot reproduce i3's `open_window(rect => [x, y, width, height])`
input or later X11 `Window::rect(Rect)` configure requests. The i3 helper passes
the initial rectangle when it creates an X11 child window
(`i3/testcases/lib/i3test.pm.in:313-350`), and i3 applies later configure-request
position and size fields to floating leaves (`i3/src/handlers.c:300-371`). An
`xdg_toplevel` client has no equivalent request for either operation.
`xdg_surface.set_window_geometry` describes visible bounds in surface-local
coordinates and says changing its x/y should generally not alter the window's
position (`xdg-shell.xml`, `xdg_surface.set_window_geometry`). The toplevel
`move` and `resize` requests start interactive compositor-controlled operations,
require an input-event serial, and do not accept a target size or position
(`xdg-shell.xml`, `xdg_toplevel.move`, `xdg_toplevel.resize`). Although the test
client can choose its buffer size after a compositor configure, that is not the
X11 request under test. Using a swayward window rule or compositor command would
test compositor policy instead of equivalent client input. The adapter's
rectangle setter therefore dies loudly; assertions that depend on X11 configure
requests remain unproven.

`tests/i3/passing.txt` lists the files that pass in full, and the default gate
runs every one of them. A conformance slice adds its file to that list the
moment it goes green, which keeps coverage from silently rotting and lets
slices land in parallel without editing the same Rust source.

Run the full passing set with `cargo test -p swayward i3_conformance_runner`.
Select another vendored file with, for example:

```sh
SWAYWARD_I3_TEST=122-split.t cargo test -p swayward i3_conformance_runner -- --nocapture
```

Passing files keep the adapter wired into the normal test gate. Other vendored
files intentionally retain their failing assertions: those failures are
conformance findings, not expectations to bless or silently skip. See the task
report for assertion-level results.

The manifest runner wraps every file with its filename, including setup,
configuration, control-socket, rejected-command allow-list, and timeout panics;
TAP failures additionally retain their assertion and source-line summary. A
residual full-suite failure observed after the duplicate-mark fix could not be
reproduced in 10 isolated manifest runs or 10 full-suite runs under the required
2 GiB memory cap. Isolated manifest runs took 36.27–36.64 seconds after warm-up;
full-suite runs took 42.15–43.08 seconds for the swayward test binary. The
original 62-second failure remains unexplained, so the timeout remains 30
seconds per vendored file rather than being raised without evidence.

## Coverage

The status `skip: i3-only tree structure` applies when a test requires the i3
output-level `content` container. Sway places workspaces directly below outputs
(`sway/sway/ipc-json.c:869-874`), so the adapter cannot expose that i3 node
without fabricating a tree that real sway clients do not see. See
[Known deviations from sway](../../docs/KNOWN_DEVIATIONS.md#output-content-nodes).

| File | Assertions | Status | Reason |
| --- | ---: | --- | --- |
| `111-goto.t` | 13 | pass | Mark focus preserves focus when no container matches, selects a marked container on the current workspace, and switches workspaces for a remote match. The unmatched criterion returns sway's `No matching node.` failure (`sway/sway/commands.c:301-304`). |
| `118-openkill.t` | 6 | pass with portability substitution | Opens and closes the focused window, then closes an unfocused window by container id. `open` uses the adapter's documented real-Wayland-window substitution. |
| `120-multiple-cmds.t` | 31 | finished: 19 pass; 12 skip | The twelve `kill; kill` variants rely on i3's X11 client exiting between commands. Sway executes both commands before processing client events and sends both asynchronous close requests to the still-focused Wayland view (`sway/sway/commands.c:288-329`; `sway/commands/kill.c:9-30`; `sway/tree/view.c:546-550`; `sway/desktop/xdg_shell.c:252-256`). A real sway 1.11 run left one of two `foot` windows mapped after `kill; kill`, matching swayward and the adapter. The close assertions are skipped as i3-only timing; the adapter removes the surviving test windows between variants so the eleven formerly contaminated setup assertions now pass. The parser and invalid-command assertions also pass. |
| `122-split.t` | 31 | 31 pass; remainder skip: i3-only tree structure | Singleton stacked assertions 28 and 30 pass. The remainder starts by inspecting i3's `content` node at line 157, which sway does not have (`sway/sway/ipc-json.c:869-874`). |
| `126-regress-close.t` | 1 | pass with stale-setup caveat | The liveness assertion passes, but obsolete `mode toggle` is rejected, so the file does not exercise its intended floating close. At the pinned i3 revision, `mode` selects a binding mode (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`), while `floating toggle` is the current command (`i3/parser-specs/commands.spec:260-262`). Sway agrees (`sway/commands/mode.c:23-62`). A temporary corrected-command diagnostic passes. |
| `127-regress-floating-parent.t` | 4 | unproven | The file's two obsolete `mode toggle` commands cannot create and later restore the floating container whose parent-removal sequence it intends to test. At the pinned i3 revision, `mode` selects a binding mode (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`), while floating state uses `floating toggle` (`i3/parser-specs/commands.spec:260-262`). Sway agrees (`sway/commands/mode.c:23-80`). The unchanged assertions pass 4/4 but are not evidence for the intended regression; a temporary corrected-command diagnostic passes 4/4. |
| `130-close-empty-split.t` | 8 | pass | Container splits retain leaf focus and collapse after their children close or move, matching `sway/tree/container.c:1590-1616`. |
| `141-resize.t` | 84 | finished: 83 pass; 1 skip | Assertion 84 traverses the child of i3's floating wrapper. All direct tiled and floating resize assertions pass. See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `116-nestedcons.t` | 7 | 4 pass; 3 skip | The root-schema assertion expects i3-only fields that sway omits, and workspace discovery assumes i3’s output-level `content` container. Sway’s common node serializer stops at `scratchpad_state`, and sway places workspaces directly below outputs (`sway/sway/ipc-json.c:258-296,838-874`). A temporary direct-workspace diagnostic passes the remaining three assertions. |
| `145-flattening.t` | 8 | pass | Directional moves preserve the source wrapper until destination attachment and then squash the completed workspace, matching `sway/commands/move.c:395-413`; layout commands retain their separate one-level flattening behavior. |
| `146-floating-reinsert.t` | 3 | pass | A window floated from a nested split retains its tiling parent and returns to that split, matching sway’s floating command targeting (`sway/commands/floating.c:23-55`). |
| `155-floating-split-size.t` | 4 | unproven | The file depends on X11 client-requested `rect` sizes and expects `focus parent; floating enable` to float the whole split. The Wayland adapter cannot provide the requested geometry, and sway floats the selected parent container (`sway/commands/floating.c:23-55`); swayward’s focus-only tiling root has no window that its floating API can move. |
| `142-regress-move-floating.t` | 1 | unproven | The obsolete `mode toggle` command leaves the window tiled, so the liveness assertion does not test moving a floating window between workspaces. Both pinned i3 and sway treat `mode` as binding-mode selection rather than floating state (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`; `sway/commands/mode.c:23-80`). The unchanged assertion passes 1/1 but is not evidence for the intended regression; a temporary `floating toggle` diagnostic passes 1/1. |
| `144-regress-floating-resize.t` | 1 | pass with stale-setup caveat | The width assertion passes, but obsolete `mode toggle` is rejected, so the file does not exercise closing a floating child. See `126-regress-close.t`; a temporary `floating toggle` diagnostic passes. |
| `147-regress-floatingmove.t` | 2 | unproven | The obsolete `mode toggle` command leaves the focused parent tiled, so neither liveness assertion tests movement out of a floating container. Both pinned i3 and sway treat `mode` as binding-mode selection rather than floating state (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`; `sway/commands/mode.c:23-80`). The unchanged assertions pass 2/2 but are not evidence for the intended regression; a temporary `floating toggle` diagnostic passes 2/2. |
| `151-regress-float-size.t` | 1 | unproven | The file's two obsolete `mode toggle` commands omit both the floating and return-to-tiling transitions, so its liveness assertion tests only opening two tiled windows. Both pinned i3 and sway treat `mode` as binding-mode selection rather than floating state (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`; `sway/commands/mode.c:23-80`). The unchanged assertion passes 1/1 but is not evidence for the intended regression; a temporary corrected-command diagnostic passes 1/1. |
| `152-regress-level-up.t` | 1 | pass with stale-setup caveat | The liveness assertion passes, but obsolete `mode toggle` is rejected, so the file does not exercise toggling the content-level selection. See `126-regress-close.t`; a temporary `floating toggle` diagnostic passes. |
| `115-ipc-workspaces.t` | 8 | pass | Creating and switching to an empty workspace emits sway's `init`, `focus`, and `empty` sequence with full workspace nodes (`sway/tree/workspace.c:268,301`; `sway/sway/ipc-server.c:295-320`). |
| `117-workspace.t` | 92 | finished: 89 pass; 3 skip | Negative workspace prefixes now serialize as `num: -1`, matching sway (`sway/sway/ipc-json.c:503-517`). The upstream file stalls at its i3-only output `content` lookup (`sway/sway/ipc-json.c:869-874`); direct workspace nodes prove assertions 51, 57, 63, and 69, while assertion 75 cannot compare the same hierarchy. Assertions 81 and 92 expect i3 rename parsing and case-only spelling changes that sway deliberately does not perform (`sway/commands/rename.c:36-38,66-92`). |
| `176-workspace-baf.t` | 26 | unproven | The file launches two compositor configurations, changes `workspace_auto_back_and_forth`, and restarts i3. Rebuilding `Fixture` destroys its Wayland clients and windows; `State::reload_config` preserves them and is not a compositor restart. Substituting either lifecycle would change the test's input. |
| `174-border-config.t` | 13 | finished: 11 pass; 2 skip | The deprecated `new_window` and `new_float` aliases map to per-window initial border rules without changing swayward's shipped 4px default. Assertions 10 and 13 traverse the child of i3's floating wrapper; a temporary direct-leaf diagnostic passes both and confirms the normal/2px and pixel/1px states. Sway serializes floating leaves directly (`sway/sway/ipc-json.c:532-540`); see [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `178-regress-workspace-open.t` | 1 | pass | An inactive named workspace is removed after its final window closes. |
| `179-regress-multiple-ws.t` | 6 | pass | Relative `move workspace prev` resolves against sway's global workspace order before moving. |
| `181-regress-float-border.t` | 6 | 1 pass; 5 unproven | The initial X11-requested 200×100 geometry is unavailable to native `xdg_toplevel`, and the resulting wrong baseline invalidates four size checks plus the final whole-rectangle comparison after changing border width. The fullscreen transition preserves the baseline rectangle. The file additionally uses i3's legacy `border 1pixel` spelling, which sway rejects; the supported equivalent `border pixel 1` and floating-node metadata are covered by native tests. |
| `185-scratchpad.t` | 93 | 16 pass; remainder unproven | The unmodified file stops after 2 assertions at i3's output-level `content` node. A temporary direct-node diagnostic reaches assertion 19. Later sections require X11 client-requested geometry, restart persistence, and i3 floating wrappers, which the Wayland fixture cannot reproduce (`sway/sway/ipc-json.c:463-492,532-540`; `sway/sway/tree/root.c:99-233`). A temporary two-output direct-node diagnostic proves that `move output left` parses and now resolves the real output directly to the left from the container's current output (`sway/commands/move.c:519-525`). `move output __i3` parses and returns sway's unknown-output failure because sway's `__i3` node is synthetic and omitted from `GET_OUTPUTS` (`sway/sway/ipc-json.c:459-499`). |
| `187-commands-parser.t` | 25 | skip: i3 parser internals | Every assertion invokes i3's standalone `test.commands_parser` binary and compares its generated-parser callback trace or exact diagnostic text. The harness has no such binary, so all 25 receive `command not found`. Sway dispatches commands through its own handler tables and `split_args` path rather than i3's generated parser (`sway/commands.c:151-176,250-329`); reproducing i3 callback names and diagnostics would not test sway-compatible runtime behavior. |
| `190-scratchpad-diff-ws.t` | 3 | pass | Criteria-targeted `move scratchpad` and `scratchpad show` operate on the matched window across workspaces, matching sway's overridden-node handling (`sway/commands/move.c:921-947`; `sway/commands/scratchpad.c:75-89,114-128`). |
| `191-resize-levels.t` | 3 | pass | Container split preserves the selected branch, so directional resize skips an unusable inner boundary and reaches the ancestor boundary, matching `sway/commands/resize.c:45-72`. |
| `192-layout.t` | 34 | pass | Supports default, `all`, and custom layout-toggle cycles; list forms skip unknown entries as sway does (`sway/commands/layout.c:47-95`). Sway rejects the i3 no-op `layout toggle stacked` (`layout.c:57-71`); see `docs/KNOWN_DEVIATIONS.md`. |
| `193-ipc-version.t` | 4 | finished: 3 pass; 1 skip | The integer minor and patch checks pass. Assertion 1 requires i3 major version 4, but sway's six-field `GET_VERSION` reply reports the compositor's own identity and version (`sway/sway/ipc-json.c:225-238`). Swayward follows that schema and reports its own version; see [Version identity](../../docs/KNOWN_DEVIATIONS.md#version-identity). |
| `197-regression-move-vanish.t` | 2 | pass | Moving a child from a split preserves both windows. |
| `198-regression-scratchpad-crash.t` | 1 | pass | Moving and immediately showing an invisible window does not crash. |
| `199-ipc-mode-event.t` | 1 | pass | Selecting the configured `m1` binding mode emits one mode event. Its payload shape is pinned by the real sway fixtures, including the `pango_markup` field (`tests/fixtures/sway/events/mode.resize.json`; `sway/commands/mode.c:71-80`). |
| `003-ipc.t` | 1 | pass | Switching from a workspace with a window to a fresh workspace changes keyboard focus. |
| `204-regress-scratchpad-move.t` | 1 | pass | Moving the last window of an inactive workspace to scratchpad does not crash. |
| `205-ipc-windows.t` | 3 | unproven | Sway emits both `new` and `focus` while mapping a newly focused view (`sway/tree/view.c:896-903`; `sway/input/seat.c:1189-1198`), but swayward emits only `new`. The fixtures pin each payload shape (`tests/fixtures/sway/events/window.new.json` and `window.focus.json`) but do not pin this sequence. A capture from real sway must establish event order and multiplicity before this test can classify the behavior. |
| `224-regress-resize-branch.t` | 1 | pass | `does_i3_live` after resizing a split container. |
| `227-ipc-workspace-empty.t` | 8 | pass | Switching from an empty workspace and closing the final window on an invisible workspace emit sway's ordered `focus` and `empty` events; switching from a non-empty workspace emits only `focus` (`sway/tree/workspace.c:301`; `sway/sway/ipc-server.c:295-320`). |
| `273-regress-focus-toggle.t` | 1 | pass | `does_i3_live` after `focus mode_toggle` on an empty workspace; sway implements this command in `sway/commands/focus.c:422`. |
| `292-regress-layout-toggle.t` | 1 | pass | `does_i3_live` after invalid `layout toggle` parameters; sway validates the accepted syntax in `sway/commands/layout.c:25-27`. |
| `299-regress-scratchpad-focus.t` | 1 | pass | Showing a scratchpad window from another workspace moves and focuses it, matching `sway/tree/root.c:157-200`. |
| `303-regress-move-floating.t` | 3 | pass | Moving a nested floating container leaves two tiled nodes and no floating node. |
| `173-get-marks.t` | 3 | pass | `GET_MARKS` starts empty, includes a new mark, and drops the mark when its window closes. |
| `210-mark-unmark.t` | 17 | finished: 14 pass; 3 skip | Test-side Wayland identity translation proves assertions 7–13 and 16. Assertions 14, 15, and 17 expect i3 to reject one mark applied to several matches. Sway instead runs the command for each match and moves the duplicate mark to the last container (`sway/sway/commands.c:301-326`, `sway/sway/commands/mark.c:46-58`); the translated `instance` criterion and node identity expose that last-match-wins result. X11 identity remains unproven (`sway/sway/ipc-json.c:670-683`). |
| `119-match.t` | 27 | pass with portability substitution | The adapter maps X11 `class` to Wayland `app_id`; sway evaluates those properties on different view types (`sway/sway/criteria.c:243-259,355-390`), so this proves matching behavior but not X11 class matching. The UTF-8 `\w` title assertion passes because criteria preserve regex backslashes, matching sway's quote-only unescape before PCRE2 compilation (`sway/sway/criteria.c:49-53,115-119,779-797,827-832`). |
| `208-regress-floating-criteria.t` | 1 | pass | The translator converts all three directives used by the test: `font`, X11-class `assign`, and X11-class `for_window`. The adapter creates the `xdg_toplevel` without mapping it until the test calls `map`; the criteria chain then runs before focus. The final X11-class focus command is rejected, but `does_i3_live` intentionally asserts only that this historical command sequence does not crash. |

## Coverage

| File | Assertions | Result | Notes |
| --- | ---: | --- | --- |
| `100-fullscreen.t` | 79 | unproven | The file depends throughout on X11-only `X11::XCB::Rect`, client fullscreen requests before mapping, and `mapped` visibility checks. Wayland clients cannot request absolute placement, and the adapter does not implement the X11 fullscreen client API; command-driven fullscreen coverage remains in native tests. |
| `101-focus.t` | 8 | pass | Directional focus follows sway's sibling traversal, ancestor escalation, and wrapping rules (`sway/commands/focus.c:158-220`). |
| `104-focus-stack.t` | 2 | pass | Closing the focused floating window restores the prior tiling focus, matching sway's focus-stack restoration (`sway/input/seat.c:260-315`). |
| `121-next-prev.t` | 12 | pass | Directional focus wraps across a workspace's horizontal children, and a criteria-targeted focus selects the requested container. |
| `124-move.t` | 54 | 49 pass; 1 skip; 4 unproven | Assertions 1–20 pass: mapping after `focus parent` resolves the focused split to its inactive view and inserts the new window as that split's sibling, matching sway (`sway/tree/view.c:850-901`); nested directional moves then descend, promote, and compact correctly (`sway/commands/move.c:112-163,300-413`). Assertions 21–40 pass with sway's 10 px default and optional custom pixel distance (`sway/commands/move.c:672-710`). Assertion 41 expects i3 percentage-point movement, while sway parses the numeric prefix and moves 25 pixels, so the percentage assertion is skipped. Assertions 45–50 pass for pixel, workspace-relative percentage, and absolute-center positions (`sway/commands/move.c:779-918`). Assertions 51–54 traverse i3-only floating wrappers; a temporary direct-node diagnostic proves the criteria-targeted position behavior, but the unchanged assertions remain unproven. |
| `128-open-order.t` | 7 | pass with portability substitution | New real Wayland windows open immediately after the focused leaf. `open_empty_con` cannot prove i3 empty-container behavior; see the adapter limitation above. |
| `129-focus-after-close.t` | 15 | finished: 13 pass; 2 skip | Assertions 5 and 6 require i3's `open` command and empty containers. Sway has no `open` entry in its command tables (`sway/sway/commands.c:44-144`), so swayward correctly rejects it. The adapter substitutes a real Wayland window for `open_empty_con`; passing assertions that use this helper do not prove empty-container behavior. See [The i3 `open` command and empty containers](../../docs/KNOWN_DEVIATIONS.md#the-i3-open-command-and-empty-containers). |
| `131-stacking-order.t` | 7 | pass | `split h` on a focused stacked container changes its singleton parent rather than the stacked container itself, preserving the stacked container's vertical focus axis (`sway/tree/container.c:1565-1582`; `sway/commands/focus.c:158-203`). |
| `132-move-workspace.t` | 160 | pass | Focused and criteria-selected parent containers move as intact subtrees, preserving internal focus and fullscreen state (`sway/commands/move.c:198-238,419-585`). Marks are globally unique across windows and split containers, so the final empty-workspace move criterion no longer finds a stale earlier mark (`sway/commands/mark.c:46-58`). The unchanged file passed 10/10 isolated runs after the fix. |
| `134-invalid-command.t` | 1 | pass | An unknown command returns a failure without terminating the compositor. Its expected `blargh!` rejection is listed in the runner's per-file allowlist. |
| `140-focus-lost.t` | 3 | pass | Focus survives a layout change. |
| `156-fullscreen-focus.t` | 64 | finished: 60 pass; 4 unproven | Fullscreen focus barriers, nested-container traversal, floating-origin restoration, direct focus unfullscreening, and global mode match sway (`sway/commands/focus.c:88-220,405-412`; `sway/tree/container.c:587-605,1200-1339`). Assertions 40–43 inspect the child count below i3's fullscreen split after workspace moves. See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `164-kill-win-vs-client.t` | 12 | finished: 11 pass; 1 skip | Bare `kill` and `kill window` close only the focused surface. Sway's handler does not validate `argc`, so `kill`, `kill window`, `kill client`, and forms with additional arguments all have the same container-targeted behavior (`sway/commands/kill.c:9-31`). The final assertion expects i3's `kill client` to destroy both windows from one X11 connection (`i3/src/commands.c:1210-1235`); sway does not implement that distinction, and the Wayland harness cannot recreate the killed X11 connection, so it is skipped. |
| `165-for_window.t` | 0 | unproven | The constant-only `X11::XCB` stub now lets the unchanged file compile and reach configuration translation, where it fails loud before assertions on 16 unsupported directives. Portable border actions and combined class-plus-title criteria translate. Remaining blockers are X11-only `instance`, `id`, `window_role`, and generated `window_type` criteria, plus missing workspace criteria and map-time `mark`/`exec` actions. |
| `167-workspace_layout.t` | 88 | blocked by translator after 5 pass | Sway retains the case-insensitive `workspace_layout default|stacking|tabbed` directive and wraps newly inserted tiling containers in the configured default layout (`sway/commands/workspace_layout.c:5-20`; `sway/tree/workspace.c:939-958,985-994`). Swayward's translator fails loud on `workspace_layout stacked`. Its KDL has a tabbed-or-normal `default-column-display` property but no stacked global default, so no exact translation currently exists. The first five default-layout assertions pass; the remaining 83 are blocked. |
| `166-assign.t` | 106 | 71 pass; 3 skip; 32 fail before target fix | All assertions run after adapter lifecycle and X11-stub additions. The translator now strips the optional `workspace` keyword and maps named output assignments to `open-on-output`. Workspace-number assignments fail loudly because window rules have no workspace-number target; silently treating the number as a workspace name would differ when an existing workspace such as `2: targetws` has that number. Sway records distinct assignment types and resolves them with `workspace_by_number` or `output_by_name_or_id` (`sway/commands/assign.c:31-49`; `sway/tree/view.c:631-659`). Relative and primary/nonprimary output assertions are i3-only: sway’s `output_by_name_or_id` matches only `*`, output identifiers, and names (`sway/desktop/output.c:42-63`). The three dock-state assertions are skipped because native Wayland clients cannot create X11 dock windows. |
| `169-border-toggle.t` | 20 | finished: 18 pass; 2 skip | Runtime `border none`, `normal`, and `pixel` update rendering and GET_TREE metadata, including optional thickness (`sway/commands/border.c:13-99`; `sway/sway/ipc-json.c:755-761`). Toggle cycles normal → none → pixel → normal for these adapter clients because they do not create an xdg-decoration object; native tests cover sway's additional normal → CSD state when one is present. Assertions 3–4 use i3's legacy `border 1pixel` spelling, which sway rejects; sway's equivalent is `border pixel 1`, so those assertions are skipped. Explicit `border csd` fails honestly for clients without an xdg-decoration object. |
| `170-force_focus_wrapping.t` | 12 | pass | The translator maps the deprecated directive to sway’s modern modes: true becomes `focus_wrapping force`, while false becomes `yes` (`sway/commands/force_focus_wrapping.c:6-23`; `sway.5.scd:743-750`). |
| `186-regress-assign-focus-parent.t` | 6 | pass | Title-regex assignment places each window on the destination workspace while preserving parent-focused insertion (`sway/criteria.c:203-217,601-650`; `sway/tree/view.c:631-664`). |
| `189-floating-constraints.t` | 28 | blocked by translator; mixed unproven and portable assertions | The translator fails loud before assertions on `floating_minimum_size`. Sway implements global minimum and maximum dimensions, including `-1` and automatic limits (`sway/commands/floating_minmax_size.c:9-52`; `sway/tree/container.c:793-830`). Assertions 1–14 depend on unavailable X11 initial rectangle requests. Assertions 15–16 compare position before and after a portable resize command and can run once the earlier setup is bypassed. Assertions 17–20 use absolute `resize set` commands and can test the global limits without relying on the initial size. Assertions 21–28 use X11 `WM_NORMAL_HINTS`; native `xdg_toplevel.set_min_size` and `set_max_size` could provide equivalent size constraints, but the harness does not expose them. |
| `236-floating-focus-raise.t` | 6 | 6 unproven | All assertions traverse the child of an i3 floating wrapper. A temporary direct-node diagnostic passes 6/6, and real sway captures plus a mutation-verified native test prove that directional focus raises the selected float and serializes floating nodes back-to-front (`sway/commands/focus.c:475-486`; `sway/tree/container.c:1682-1692`). See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `237-regress-assign-focus.t` | 1 | pass | Title-based assignment plus `for_window ... layout tabbed, focus` executes without crashing (`sway/criteria.c:203-217,601-650`; `sway/tree/view.c:570-590,631-664`). |
| `005-floating.t` | 13 | finished: 6 pass; 7 unproven | Assertions 5 and 7–13 depend on i3's X11 `rect` creation input. Wayland `xdg_toplevel` has no equivalent absolute-position request; using a window rule or compositor move would test a different input. Sway clamps and centers natural floating geometry (`sway/tree/container.c:793-905,955-982`). See the adapter limitation above. |
| `135-floating-focus.t` | 82 | finished: 66 pass; 16 skip | Assertions 23–25 require distinct X11 positions that the Wayland adapter cannot request. Assertions 31, 32, 34, 35, 37, 40, 43, 46, 50, 54, 58, 62, 66, 70, 73, and 74 use i3's floating wrappers, X11-only `window` field, or opposite floating-list insertion order; equivalent direct-node checks pass where the hierarchy agrees. Layer focus modes, workspace child descent, cross-workspace focus, nested reinsertion, and close restoration match sway. Assertions 75 and 77 pass through a temporary direct-node diagnostic after preserving non-root parents across floating transitions (`sway/tree/container.c:955-1013`). Assertions 50 and 58 expect i3's new floating wrapper at index 0, while sway appends new floating containers (`sway/tree/workspace.c:961-971`). See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `112-floating-resize.t` | 11 | unproven | The unchanged file executes 0 assertions and stops at its first `Window::rect(Rect)` call with `X11 window geometry mutation is unavailable in the Wayland test adapter`. Assertions 1–9 depend on X11 configure requests that set a floating window's position and size; assertions 10–11 depend on an out-of-bounds position request. Native `xdg_toplevel` has no equivalent request: `set_window_geometry` is surface-local and must not be treated as desktop placement, while `move` and `resize` are serial-gated interactive operations without target coordinates or dimensions (`xdg-shell.xml`, `xdg_surface.set_window_geometry`, `xdg_toplevel.move`, `xdg_toplevel.resize`). Client buffer resizing after `ack_configure` would answer a different question. All 11 assertions are therefore unproven, and the fail-loud setter is retained. |
| `138-floating-attach.t` | 11 | pass | Opening a tiled window when only a float exists creates a tiled root, and opening after a float over a stacked layout preserves that stacked container. No setup command is rejected. |
| `148-regress-floatingmovews.t` | 1 | unproven | The unchanged file uses obsolete `mode toggle`, which both pinned i3 and sway interpret as binding-mode selection rather than floating state (`i3/parser-specs/commands.spec:480-482`; `sway/commands/mode.c:23-62`). The rejected setup leaves the second window tiled. A temporary `floating toggle` diagnostic still fails because moving the focused float away restores focus to the prior tiled window, matching sway's focus-restoration path (`sway/commands/move.c:554-608`); the assertion's expected remote focus is i3-only. |
| `153-floating-originalsize.t` | 7 | unproven | All assertions depend on `open_window(rect => ...)`, an X11 client size request that the Wayland adapter cannot reproduce. The first three observe the adapter's 1×1 default stretched by tiling, and the final four compare the float against the unavailable 400×150 request, so none proves original-size restoration. |
| `136-floating-ws-empty.t` | 11 | pass | An inactive workspace remains present while it contains floating windows, matching sway's emptiness check over both tiled and non-sticky floating children (`sway/tree/workspace.c:752-764`). |
| `137-floating-unmap.t` | 2 | pass | A floating window becomes invisible after its workspace loses visibility, matching `sway/tree/view.c:1149-1172`. |
| `139-ws-numbers.t` | 8 | 3 pass; 5 fail: deferred workspace model | All commands parse. Number serialization and lookup pass, but workspaces remain in creation order instead of sway's numeric-first insertion order (`sway/sway/tree/workspace.c:255-259`). This is the already-documented workspace identity/lifecycle design issue, not a new defect. |
| `219-ipc-window-focus.t` | 30 | pass | All 30 real assertions pass (10 top-level TAP results because nine are subtests). Window `focus`, `close`, and scratchpad `move`/`focus` event sequences match sway (`sway/input/seat.c:1197`; `sway/tree/container.c:494`; `sway/commands/scratchpad.c:60-89`). |
| `254-move-to-output-with-criteria.t` | 16 | finished: 14 pass; 2 skip | `fresh_workspace(output => N)` selects each real output through sway's `focus output <direction|name>` command. Assertions 14 and 16 expect i3 to cycle criteria matches across the supplied output list. Sway instead accepts the extra arguments but resolves only the first output name for every match (`sway/commands/move.c:419-425,519-525`), placing both windows on `fake-1`. |
| `285-sticky.t` | 11 | pass | Sticky floating windows follow workspace switches on their output, preserve focus according to the destination workspace, and ignore switches to other outputs, matching sway (`sway/commands/sticky.c:15-40`; `sway/input/seat.c:1209-1222`). Tiling windows may carry the flag but do not move because sway considers stickiness active only for floating containers (`sway/tree/container.c:1705-1711`). |
| `242-no-focus.t` | 6 | pass with portability substitution | Portable `no_focus` criteria translate to the existing `open-focused false` window-rule property. The adapter's documented X11 `instance` to Wayland `app_id` substitution lets the unchanged test prove that a matching second window does not take focus, a matching first window does, and a later matching float does not. Sway exempts the first window on a workspace from `no_focus` (`sway/commands/no_focus.c:7-34`; `sway/tree/view.c:697-732`). The shared translator still rejects X11-only `instance` outside the adapter substitution. |
| `251-command-criteria-focused.t` | 11 | finished: 9 pass; 2 unproven | `__focused__` matching passes for the adapter's Wayland `app_id`, title, and workspace properties, including the no-focused-window liveness check. Assertion 4 needs an X11 instance distinct from class; native `xdg_toplevel` has only `app_id` (`xdg-shell.xml`, `xdg_toplevel.set_app_id`). Assertion 8 separately needs the X11 `WM_WINDOW_ROLE` property set by its `before_map` callback. Sway evaluates both properties only in its Xwayland criteria path and emits their IPC fields only for Xwayland views (`sway/criteria.c:355-410`; `sway/sway/ipc-json.c:670-700`). Swayward delegates X11 to `xwayland-satellite`, which presents ordinary Wayland surfaces and does not give this harness those three separate identities. Testing them requires a metadata bridge plus production criteria and IPC support, the satellite and an X11 client in the headless environment, and new lifecycle handling in the harness. It is not an adapter alias. |
| `246-window-decoration-focus.t` | 3 | pass | All three assertions exercise real pointer motion and rendered-geometry hit testing with sway's default focus-follows-mouse behavior. Ordinary decoration entry focuses its leaf. Tabbed and stacked titlebars resolve to the represented branch's focused descendant before activation, matching sway's visible-view focus rule (`sway/input/seatop_default.c:538-605`). |
| `240-focus-on-window-activation.t` | 15 | pass with protocol substitution | The adapter maps the test's activation-message shape to a real serial-less xdg-activation token and activation request for the same native Wayland surface. All 15 assertions pass: `urgent` preserves focus and marks the target urgent on visible and hidden workspaces; `focus` focuses the target without urgency on both; `none` changes neither. The translator maps those modes to `set-urgent`, `focus`, and `ignore` window rules. Sway's distinct `smart` mode focuses only on a visible workspace and otherwise sets urgency (`sway/commands/focus_on_window_activation.c:3-24`; `sway/tree/view.c:477-512`); it remains fail-loud because no single window-rule value represents it. |
| `203-regress-assign-and-move.t` | 2 | pass with portability substitution | The adapter maps X11 `instance` to Wayland `app_id`. Assignment followed by a `for_window` workspace move, and two distinct `for_window` workspace moves, do not crash. Sway selects the assigned workspace before mapping and then runs matching `for_window` commands against the new container (`sway/tree/view.c:628-665,942`; `sway/tree/view.c:569-588`). |
| `232-cmd-move-criteria.t` | 22 | 12 pass; 10 unproven | Criteria targets are materialized once per command list, then directional move runs for each target without changing focus, matching sway (`sway/commands.c:232-326`; `sway/commands/move.c:672-718`). The `id` movement and focus-preservation assertions pass. The other ten movement assertions require X11 `window_type`; native Wayland clients cannot set that property, so their corresponding focus checks pass but movement remains unproven. |
| `243-move-to-mark.t` | 50 | 43 pass; 2 unproven; 5 skip | The unchanged file passes its first 11 assertions, then stops at an X11 urgency client message that the Wayland adapter cannot perform; the target-workspace urgency assertion is unproven. A temporary diagnostic that bypasses only the unavailable urgency event and unrelated stale `focus <workspace>` and `splitv` spellings passes assertions 1–12, 14–39, 41, 42, 45, and 47–50. Moving to a marked leaf inserts the source as its next sibling, while moving to a marked split appends the source as a child, matching sway (`sway/commands/move.c:241-270,419-625,956`). Assertion 40 traverses i3's floating wrapper and is unproven. Assertions 43, 44, 46, 47, and 50 depend on marks applied to workspaces, which sway rejects because `mark` requires a container (`sway/commands/mark.c:15-23`); their setup and dependent shape checks are skipped. |
| `274-move-branch-position.t` | 16 | pass | Directional moves prepend into parallel branches for right/down movement and insert after the focused inactive leaf in perpendicular branches. Same-parent leaf moves remain swaps. New children receive equal shares, matching sway's zero-fraction normalization (`sway/commands/move.c:80-89,112-165`; `sway/tree/arrange.c:16-52`). |
| `184-regress-float-split-resize.t` | 1 | unproven | The liveness assertion passes, but `focus parent; floating toggle` selects a split container. Sway can float that tree node (`sway/commands/floating.c:23-55`); swayward stores only floating leaves, as documented under [Floating split containers](../../docs/KNOWN_DEVIATIONS.md#floating-split-containers), so the intended resize path does not execute. |
| `194-regress-floating-size.t` | 15 | unproven | Every size assertion depends on `open_window(rect => [0, 0, 400, 150])`, an X11 client size request unavailable to native `xdg_toplevel`. The unchanged file reaches three assertions, then stops when its next deprecated `new_window` directive fails translation. |
| `211-regress-urgency-assign.t` | 3 | unproven | The file reaches its first assertion, then calls the X11-only `add_hint('urgency')` API. Native Wayland clients cannot set the X11 urgency hint; sway handles that request only through its Xwayland view path (`sway/tree/view.c:1197-1224`). |
| `218-regress-floating-split.t` | 2 | skip | Both assertions pass, but sway rejects `layout stacked` for a floating container (`sway/commands/layout.c:126-132`) while i3 accepts the setup as a no-op. Swayward follows sway and reports that rejection, so the file cannot enter the green manifest. |
| `233-regress-manage-focus-unmapped.t` | 2 | pass with portability substitution | The translator maps X11 `class` to Wayland `app_id` and preserves `kill` and `move scratchpad` as one-shot map-time sway commands. Both focus-preservation assertions run and pass: hiding or closing the newly mapped matching window leaves the prior window focused. Sway likewise executes every matching `for_window` command after inserting the view but before deciding whether to focus it (`sway/tree/view.c:569-588,942-944`). Swayward snapshots the matching command strings but resolves the target by `con_id` again for each action, so it never reuses a stale materialized target. |
| `225-ipc-window-fullscreen.t` | 4 | pass | Both nested event assertions run for each transition: one `window` event is emitted, with `container.fullscreen_mode` changing to 1 and then 0. Sway serializes the container's pending fullscreen enum directly (`sway/sway/ipc-json.c:740-742`), and the captured sway event records mode 1 (`tests/fixtures/sway/events/window.fullscreen_mode.json`). |
| `206-fullscreen-scratchpad.t` | 8 | 6 pass; 1 unproven; 1 fail | Assertions 1, 2, 4–6, and 8 pass without a panic. Assertion 3 follows a rejected `layout tabbed`: the shown scratchpad remains a floating leaf, while the test needs i3's floating wrapper to select and fullscreen a parent. Sway can operate on a floating split root (`sway/commands/floating.c:23-55`), but swayward's floating-split model is deferred, so that assertion is unproven. Assertion 7 exposes a swayward bug: showing a criteria-selected scratchpad window leaves the current workspace fullscreen active. Sway disables workspace and global fullscreen before showing the scratchpad container (`sway/sway/tree/root.c:157-173`). |
| `202-scratchpad-criteria.t` | 27 | 20 pass; 7 fail | Criteria matching, focus restoration, non-scratchpad no-op behavior, and the initial two-window show pass. Assertions 3–6 expose missing sway default scratchpad sizing: sway converts a tiled container to floating, applies its default size, and centers it when first moved to scratchpad (`sway/sway/tree/root.c:109-125`); swayward reports the native client's 9×9 size. Assertions 21, 25, and 27 expose broken criteria toggling/cycling with multiple scratchpad windows. Sway applies the overridden-node toggle independently to each matched scratchpad container (`sway/sway/commands/scratchpad.c:75-89`). Expected no-match and non-scratchpad command failures are reported honestly by the adapter. |
| `213-layout-restore-simple.t` | 18 | 8 pass; 10 skip | All ten layout-shape and swallow assertions depend on i3's `append_layout`, which sway does not implement: it is absent from sway's complete command tables and runtime command reference (`sway/sway/commands.c:44-144`; `sway/sway/sway.5.scd:102-415`). The eight setup and liveness assertions pass around the expected rejection but do not prove layout restoration. See [Layout restoration](../../docs/KNOWN_DEVIATIONS.md#layout-restoration). |
| `272-regress-focus-assign.t` | 8 | 7 pass; 1 unproven | Swayward now creates a missing named assignment workspace before initial configure and maps the window there without changing focus, matching sway's assignment-before-map and focus-after-criteria order (`sway/tree/view.c:628-665,942-945`). The final placement assertion expects X11 `window_type=utility` to make the second window floating; native Wayland clients cannot set that property, so it is unproven. |
| `279-regress-default-floating-border.t` | 1 | blocked by translator | Deprecated `new_window pixel 5` and `new_float normal` directives stop translation before the assertion. Sway aliases them to `default_border` and `default_floating_border` (`sway/commands/new_window.c:5-12`; `sway/commands/new_float.c:5-12`). The assertion also traverses i3's floating wrapper, so direct-node verification is required after translation support exists. |
| `281-regress-reload-bindsym.t` | 1 | unproven | The unchanged file does not compile because the adapter does not export Test::More's `skip`; its only action uses X11 `xdotool` to synthesize a key press, which the headless Wayland harness cannot reproduce. |
| `296-regress-focus-behind-fullscreen-floating.t` | 1 | pass | Directional focus across outputs returns to the existing floating fullscreen window when that workspace has no tiled window. |
| `312-regress-layout-default.t` | 0 | unproven | Upstream contains no TAP assertions. The adapter's zero-assertion guard rejects the file, so it cannot provide evidence even though both commands execute. |
| `517-regress-move-direction-ipc.t` | 6 | skip: i3-only workspace event | Real sway 1.11 emits no workspace event when `move right` carries the focused window to an adjacent output, whether the destination workspace is empty or occupied and whether the moved window is the source workspace's last window. Captured ordered streams are empty in all three cases. Sway reparents the container without calling `seat_set_focus`, so `set_workspace` cannot emit the focus event that i3 expects (`sway/commands/move.c:168-190,276-299,672-744`; `sway/input/seat.c:1098-1113`). |
| `520-regress-focus-direction-floating.t` | 1 | pass | `mouse_warping none` maps to swayward's disabled `warp-mouse-to-focus` setting. The portable assertion proves that directional focus crossing outputs selects the existing floating window when the source output has no tiling window, matching sway's directional focus traversal (`sway/commands/focus.c:240-262`). |
| `143-regress-floating-restart.t` | 5 | unproven | The unchanged assertions pass around a rejected `restart`, so no compositor state crosses an in-place restart. The harness cannot reproduce that lifecycle. |
| `150-regress-dock-restart.t` | 11 | unproven | The test combines X11 EWMH dock windows with in-place restart. The Wayland harness provides neither. |
| `154-regress-multiple-dock.t` | 2 | unproven | The assertions pass only because `get_dock_clients` is empty and native test windows are not X11 docks; the intended dock-destruction path is not exercised. |
| `161-regress-borders-restart.t` | 4 | unproven | The test requires an in-place restart. Its setup also uses i3's `border 1pixel` spelling, which sway rejects in favor of `border pixel 1` (`sway/commands/border.c:13-99`). |
| `162-regress-dock-urgent.t` | 4 | unproven | X11 dock creation and the X11 urgency hint API are unavailable to native Wayland test clients. |
| `168-regress-fullscreen-restart.t` | 1 | unproven | Its sole liveness assertion follows a rejected `restart`; the harness cannot test state across an in-place compositor restart. |
| `182-regress-focus-dock.t` | 1 | unproven | The liveness assertion passes, but the native test client cannot create the X11 EWMH dock needed by the regression. |
| `188-regress-focus-restart.t` | 11 | unproven | All assertions pass around a rejected `restart`; the harness cannot test focus state across an in-place compositor restart. |
| `222-regress-dock-resize.t` | 1 | unproven | The liveness assertion passes, but the native client does not become an X11 EWMH dock, so the dock-resize path is not exercised. |
| `248-regress-urgency-clear.t` | 4 | unproven | Configuration translation stops before assertions, and the test requires an X11 `_NET_ACTIVE_WINDOW` client message. The native Wayland harness cannot generate that input. |
| `267-regress-mark-restart.t` | 1 | unproven | Its sole liveness assertion follows a rejected `restart`; the harness cannot test marks across an in-place compositor restart. |

The pinned oracle has 23 unvendored `regress` files after this batch. Five are dock-dependent (`150-regress-dock-restart.t`, `154-regress-multiple-dock.t`, `162-regress-dock-urgent.t`, `182-regress-focus-dock.t`, and `222-regress-dock-resize.t`), and six require an in-place compositor restart (`143-regress-floating-restart.t`, `150-regress-dock-restart.t`, `161-regress-borders-restart.t`, `168-regress-fullscreen-restart.t`, `188-regress-focus-restart.t`, and `267-regress-mark-restart.t`). Those classes are unproven by this Wayland harness. `150` belongs to both classes, leaving 13 other reachable candidates. The pinned checkout has `248-regress-urgency-clear.t`, not `248-regress-move-restart.t`.

The initial X11-protocol exclusions are `113-urgent.t`,
`162-regress-dock-urgent.t`, `196-randr-output-names.t`,
`209-ewmh-net-workarea.t`, `234-ewmh-desktop-names.t`,
`277-ipc-window-urgent.t`, `284-ewmh-visible-name.t`,
`294-update-ewmh-atoms.t`, `521-ewmh-desktop-viewport.t`, and `533-randr15.t`.

Tests that inspect X11, EWMH, RandR, XKB, or raw X events are outside this
adapter's scope. Tests requiring unsupported i3 commands such as
`append_layout` are added only when swayward implements the corresponding sway
command.
