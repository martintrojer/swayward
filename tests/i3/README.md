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
same `node.id`. It does not add `window` to the IPC object: key iteration and
`exists` still expose sway's native Wayland schema, which omits `window`
(`sway/sway/ipc-json.c:670-683`). These comparisons prove container identity and
the behavior addressed through it, but do not prove X11 window identity or
Xwayland serialization.

For an `i3_config` import, the adapter sends the complete config to the Rust
runner. The runner translates it with the shipped `contrib/sway-to-kdl` tool and
refuses to run if the translator reports any manual-attention item. The adapter
maps the i3 suite's X11 `class` and `instance` criteria to Wayland `app_id`
criteria. This is a portability substitution, not sway criteria equivalence:
sway matches native Wayland views by `app_id` and evaluates `class` and
`instance` only for Xwayland views (`sway/sway/criteria.c:243-259,355-390`).
These tests therefore prove equivalent matching behavior against the identity
the adapter can assign, but do not prove X11 class or instance matching. The
translator supports `assign` and the `floating enable` or `floating disable`
subset of `for_window`. It does not silently discard unsupported directives.

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
input. The i3 helper passes that rectangle when it creates an X11 child window
(`i3/testcases/lib/i3test.pm.in:313-350`). An `xdg_toplevel` client can set its
surface-local window geometry, but it cannot request an absolute desktop
position. Its `move` request starts an interactive compositor-controlled move
and requires an input-event serial (`xdg-shell.xml`, `xdg_toplevel.move`). Using
a swayward window rule or a compositor move would make the reported position
real, but would test compositor policy rather than equivalent client input.
Assertions that depend on the requested X11 position remain unproven.

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
| `120-multiple-cmds.t` | 31 | 8 pass; 23 fail | The adapter now exports `get_workspace_names`, so all assertions run. All twelve `kill; kill` whitespace variants fail because both commands send close requests to the same still-mapped Wayland client before the adapter reaps it. Sway executes each command immediately but Wayland close remains asynchronous (`sway/sway/commands.c:205-333`; `sway/commands/kill.c:15-30`; `sway/tree/view.c:546-550`). The invalid-command checks pass and produce their expected rejections. |
| `111-goto.t` | 13 | pass | Mark focus preserves focus when no container matches, selects a marked container on the current workspace, and switches workspaces for a remote match. The unmatched criterion returns sway's `No matching node.` failure (`sway/sway/commands.c:301-304`). |
| `118-openkill.t` | 6 | pass with portability substitution | Opens and closes the focused window, then closes an unfocused window by container id. `open` uses the adapter's documented real-Wayland-window substitution. |
| `120-multiple-cmds.t` | 31 | 8 pass; 23 fail | The adapter now exports `get_workspace_names`, so all assertions run. All twelve `kill; kill` whitespace variants fail because both commands send close requests to the same still-mapped Wayland client before the adapter reaps it. Sway also sends asynchronous close requests for each command (`sway/sway/commands.c:205-333`; `sway/commands/kill.c:15-30`; `sway/tree/view.c:546-550`), so the equivalent Wayland-client timing needs direct comparison before this is classified as a swayward bug. The invalid-command checks pass and produce their expected rejections. |
| `122-split.t` | 31 | 31 pass; remainder skip: i3-only tree structure | Singleton stacked assertions 28 and 30 pass. The remainder starts by inspecting i3's `content` node at line 157, which sway does not have (`sway/sway/ipc-json.c:869-874`). |
| `126-regress-close.t` | 1 | pass with stale-setup caveat | The liveness assertion passes, but obsolete `mode toggle` is rejected, so the file does not exercise its intended floating close. At the pinned i3 revision, `mode` selects a binding mode (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`), while `floating toggle` is the current command (`i3/parser-specs/commands.spec:260-262`). Sway agrees (`sway/commands/mode.c:23-62`). A temporary corrected-command diagnostic passes. |
| `127-regress-floating-parent.t` | 4 | unproven | The file uses obsolete `mode toggle` commands for floating transitions. At the pinned i3 revision, `mode` selects a binding mode (`i3/parser-specs/commands.spec:480-482`; `i3/src/commands.c:986-991`), while `floating toggle` is the current command (`i3/parser-specs/commands.spec:260-262`). Sway likewise treats `mode toggle` as binding-mode selection (`sway/commands/mode.c:23-62`). The unchanged file therefore rejects both setup commands; a temporary corrected-command diagnostic passes all four assertions. |
| `130-close-empty-split.t` | 8 | pass | Container splits retain leaf focus and collapse after their children close or move, matching `sway/tree/container.c:1590-1616`. |
| `141-resize.t` | 84 | finished: 83 pass; 1 skip | Assertion 84 traverses the child of i3's floating wrapper. All direct tiled and floating resize assertions pass. See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `116-nestedcons.t` | 7 | 4 pass; 3 skip | The root-schema assertion expects i3-only fields that sway omits, and workspace discovery assumes i3’s output-level `content` container. Sway’s common node serializer stops at `scratchpad_state`, and sway places workspaces directly below outputs (`sway/sway/ipc-json.c:258-296,838-874`). A temporary direct-workspace diagnostic passes the remaining three assertions. |
| `145-flattening.t` | 8 | 7 pass; 1 fail | Assertion 2 expects the first `move up; move right` sequence to flatten all three windows to workspace level. Sway calls `workspace_squash` after directional promotion (`sway/commands/move.c:395-413`), so this is a swayward tree-compaction bug; the later ticket-1053 flattening sequence passes unchanged. |
| `146-floating-reinsert.t` | 3 | pass | A window floated from a nested split retains its tiling parent and returns to that split, matching sway’s floating command targeting (`sway/commands/floating.c:23-55`). |
| `155-floating-split-size.t` | 4 | unproven | The file depends on X11 client-requested `rect` sizes and expects `focus parent; floating enable` to float the whole split. The Wayland adapter cannot provide the requested geometry, and sway floats the selected parent container (`sway/commands/floating.c:23-55`); swayward’s focus-only tiling root has no window that its floating API can move. |
| `142-regress-move-floating.t` | 1 | unproven | The file's obsolete `mode toggle` setup is rejected; see `127-regress-floating-parent.t`. A temporary `floating toggle` substitution proves the crash sequence passes. |
| `144-regress-floating-resize.t` | 1 | pass with stale-setup caveat | The width assertion passes, but obsolete `mode toggle` is rejected, so the file does not exercise closing a floating child. See `126-regress-close.t`; a temporary `floating toggle` diagnostic passes. |
| `147-regress-floatingmove.t` | 2 | unproven | The file's obsolete `mode toggle` setup is rejected; see `127-regress-floating-parent.t`. A temporary `floating toggle` substitution proves both crash checks pass. |
| `151-regress-float-size.t` | 1 | unproven | The file's obsolete `mode toggle` setup is rejected; see `127-regress-floating-parent.t`. A temporary substitution proves that floating, tiling, and opening another window does not crash. |
| `152-regress-level-up.t` | 1 | pass with stale-setup caveat | The liveness assertion passes, but obsolete `mode toggle` is rejected, so the file does not exercise toggling the content-level selection. See `126-regress-close.t`; a temporary `floating toggle` diagnostic passes. |
| `115-ipc-workspaces.t` | 8 | pass | Creating and switching to an empty workspace emits sway's `init`, `focus`, and `empty` sequence with full workspace nodes (`sway/tree/workspace.c:268,301`; `sway/sway/ipc-server.c:295-320`). |
| `117-workspace.t` | 92 | finished: 89 pass; 3 skip | Negative workspace prefixes now serialize as `num: -1`, matching sway (`sway/sway/ipc-json.c:503-517`). The upstream file stalls at its i3-only output `content` lookup (`sway/sway/ipc-json.c:869-874`); direct workspace nodes prove assertions 51, 57, 63, and 69, while assertion 75 cannot compare the same hierarchy. Assertions 81 and 92 expect i3 rename parsing and case-only spelling changes that sway deliberately does not perform (`sway/commands/rename.c:36-38,66-92`). |
| `176-workspace-baf.t` | 26 | unproven | The file launches two compositor configurations, changes `workspace_auto_back_and_forth`, and restarts i3. Rebuilding `Fixture` destroys its Wayland clients and windows; `State::reload_config` preserves them and is not a compositor restart. Substituting either lifecycle would change the test's input. |
| `178-regress-workspace-open.t` | 1 | pass | An inactive named workspace is removed after its final window closes. |
| `179-regress-multiple-ws.t` | 6 | pass | Relative `move workspace prev` resolves against sway's global workspace order before moving. |
| `185-scratchpad.t` | 93 | 16 pass; remainder unproven | The unmodified file stops after 2 assertions at i3's output-level `content` node. A temporary direct-node diagnostic reaches assertion 19. Later sections require X11 client-requested geometry, restart persistence, and i3 floating wrappers, which the Wayland fixture cannot reproduce (`sway/sway/ipc-json.c:463-492,532-540`; `sway/sway/tree/root.c:99-233`). A temporary two-output direct-node diagnostic proves that `move output left` parses and now resolves the real output directly to the left from the container's current output (`sway/commands/move.c:519-525`). `move output __i3` parses and returns sway's unknown-output failure because sway's `__i3` node is synthetic and omitted from `GET_OUTPUTS` (`sway/sway/ipc-json.c:459-499`). |
| `190-scratchpad-diff-ws.t` | 3 | pass | Criteria-targeted `move scratchpad` and `scratchpad show` operate on the matched window across workspaces, matching sway's overridden-node handling (`sway/commands/move.c:921-947`; `sway/commands/scratchpad.c:75-89,114-128`). |
| `191-resize-levels.t` | 3 | pass | Container split preserves the selected branch, so directional resize skips an unusable inner boundary and reaches the ancestor boundary, matching `sway/commands/resize.c:45-72`. |
| `192-layout.t` | 34 | pass | Supports default, `all`, and custom layout-toggle cycles; list forms skip unknown entries as sway does (`sway/commands/layout.c:47-95`). Sway rejects the i3 no-op `layout toggle stacked` (`layout.c:57-71`); see `docs/KNOWN_DEVIATIONS.md`. |
| `197-regression-move-vanish.t` | 2 | pass | Moving a child from a split preserves both windows. |
| `198-regression-scratchpad-crash.t` | 1 | pass | Moving and immediately showing an invisible window does not crash. |
| `204-regress-scratchpad-move.t` | 1 | pass | Moving the last window of an inactive workspace to scratchpad does not crash. |
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
| `134-invalid-command.t` | 1 | pass | An unknown command returns a failure without terminating the compositor. Its expected `blargh!` rejection is listed in the runner's per-file allowlist. |
| `134-invalid-command.t` | 1 | pass | An unknown command returns a failure without terminating the compositor. Its expected `blargh!` rejection is listed in the runner's per-file allowlist. |
| `140-focus-lost.t` | 3 | pass | Focus survives a layout change. |
| `156-fullscreen-focus.t` | 64 | finished: 60 pass; 4 unproven | Fullscreen focus barriers, nested-container traversal, floating-origin restoration, direct focus unfullscreening, and global mode match sway (`sway/commands/focus.c:88-220,405-412`; `sway/tree/container.c:587-605,1200-1339`). Assertions 40–43 inspect the child count below i3's fullscreen split after workspace moves. See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `170-force_focus_wrapping.t` | 12 | pass | The translator maps the deprecated directive to sway’s modern modes: true becomes `focus_wrapping force`, while false becomes `yes` (`sway/commands/force_focus_wrapping.c:6-23`; `sway.5.scd:743-750`). |
| `186-regress-assign-focus-parent.t` | 6 | pass | Title-regex assignment places each window on the destination workspace while preserving parent-focused insertion (`sway/criteria.c:203-217,601-650`; `sway/tree/view.c:631-664`). |
| `236-floating-focus-raise.t` | 6 | 6 unproven | All assertions traverse the child of an i3 floating wrapper. A temporary direct-node diagnostic passes 6/6, and real sway captures plus a mutation-verified native test prove that directional focus raises the selected float and serializes floating nodes back-to-front (`sway/commands/focus.c:475-486`; `sway/tree/container.c:1682-1692`). See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `237-regress-assign-focus.t` | 1 | pass | Title-based assignment plus `for_window ... layout tabbed, focus` executes without crashing (`sway/criteria.c:203-217,601-650`; `sway/tree/view.c:570-590,631-664`). |
| `005-floating.t` | 13 | finished: 6 pass; 7 unproven | Assertions 5 and 7–13 depend on i3's X11 `rect` creation input. Wayland `xdg_toplevel` has no equivalent absolute-position request; using a window rule or compositor move would test a different input. Sway clamps and centers natural floating geometry (`sway/tree/container.c:793-905,955-982`). See the adapter limitation above. |
| `135-floating-focus.t` | 82 | finished: 66 pass; 16 skip | Assertions 23–25 require distinct X11 positions that the Wayland adapter cannot request. Assertions 31, 32, 34, 35, 37, 40, 43, 46, 50, 54, 58, 62, 66, 70, 73, and 74 use i3's floating wrappers, X11-only `window` field, or opposite floating-list insertion order; equivalent direct-node checks pass where the hierarchy agrees. Layer focus modes, workspace child descent, cross-workspace focus, nested reinsertion, and close restoration match sway. Assertions 75 and 77 pass through a temporary direct-node diagnostic after preserving non-root parents across floating transitions (`sway/tree/container.c:955-1013`). Assertions 50 and 58 expect i3's new floating wrapper at index 0, while sway appends new floating containers (`sway/tree/workspace.c:961-971`). See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `136-floating-ws-empty.t` | 11 | pass | An inactive workspace remains present while it contains floating windows, matching sway's emptiness check over both tiled and non-sticky floating children (`sway/tree/workspace.c:752-764`). |
| `137-floating-unmap.t` | 2 | pass | A floating window becomes invisible after its workspace loses visibility, matching `sway/tree/view.c:1149-1172`. |
| `219-ipc-window-focus.t` | 30 | pass | All 30 real assertions pass (10 top-level TAP results because nine are subtests). Window `focus`, `close`, and scratchpad `move`/`focus` event sequences match sway (`sway/input/seat.c:1197`; `sway/tree/container.c:494`; `sway/commands/scratchpad.c:60-89`). |
| `254-move-to-output-with-criteria.t` | 16 | finished: 14 pass; 2 skip | `fresh_workspace(output => N)` selects each real output through sway's `focus output <direction|name>` command. Assertions 14 and 16 expect i3 to cycle criteria matches across the supplied output list. Sway instead accepts the extra arguments but resolves only the first output name for every match (`sway/commands/move.c:419-425,519-525`), placing both windows on `fake-1`. |
| `285-sticky.t` | 11 | pass | Sticky floating windows follow workspace switches on their output, preserve focus according to the destination workspace, and ignore switches to other outputs, matching sway (`sway/commands/sticky.c:15-40`; `sway/input/seat.c:1209-1222`). Tiling windows may carry the flag but do not move because sway considers stickiness active only for floating containers (`sway/tree/container.c:1705-1711`). |

The initial X11-protocol exclusions are `113-urgent.t`,
`162-regress-dock-urgent.t`, `196-randr-output-names.t`,
`209-ewmh-net-workarea.t`, `234-ewmh-desktop-names.t`,
`277-ipc-window-urgent.t`, `284-ewmh-visible-name.t`,
`294-update-ewmh-atoms.t`, `521-ewmh-desktop-viewport.t`, and `533-randr15.t`.

Tests that inspect X11, EWMH, RandR, XKB, or raw X events are outside this
adapter's scope. Tests requiring unsupported i3 commands such as
`append_layout` are added only when swayward implements the corresponding sway
command.
