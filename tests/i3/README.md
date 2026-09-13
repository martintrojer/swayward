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
is not a restart. Tests that depend on state surviving a restart remain
unproven. In particular, `176-workspace-baf.t` launches two compositor
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
| `122-split.t` | 31 | 31 pass; remainder skip: i3-only tree structure | Singleton stacked assertions 28 and 30 pass. The remainder starts by inspecting i3's `content` node at line 157, which sway does not have (`sway/sway/ipc-json.c:869-874`). |
| `126-regress-close.t` | 1 | pass | `does_i3_live` after closing a floating container. |
| `130-close-empty-split.t` | 8 | pass | Container splits retain leaf focus and collapse after their children close or move, matching `sway/tree/container.c:1590-1616`. |
| `141-resize.t` | 84 | finished: 83 pass; 1 skip | Assertion 84 traverses the child of i3's floating wrapper. Sway serializes floating leaves directly, with no child (`sway/sway/ipc-json.c:532-540,854-893`), so the helper cannot find its target. All direct tiled and floating resize assertions pass. See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `144-regress-floating-resize.t` | 1 | pass | Closing a floating child does not corrupt the tiled siblings' combined width. |
| `152-regress-level-up.t` | 1 | pass | `does_i3_live` after focusing above the workspace tree. |
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
| `129-focus-after-close.t` | 15 | finished: 13 pass; 2 skip | Assertions 5 and 6 require i3's `open` command and empty containers. Sway has no `open` entry in its command tables (`sway/sway/commands.c:44-144`), so swayward correctly rejects it. The adapter substitutes a real Wayland window for `open_empty_con`; passing assertions that use this helper do not prove empty-container behavior. See [The i3 `open` command and empty containers](../../docs/KNOWN_DEVIATIONS.md#the-i3-open-command-and-empty-containers). |
| `140-focus-lost.t` | 3 | pass | Focus survives a layout change. |
| `156-fullscreen-focus.t` | 57 | 44 pass; 13 fail | The harness now creates the two real 1024×768 outputs requested by `fake-outputs`. Remaining failures expose fullscreen focus/move behavior, including the rejected `fullscreen global`; report those separately. |
| `005-floating.t` | 13 | finished: 6 pass; 7 unproven | Assertions 5 and 7–13 depend on i3's X11 `rect` creation input. Wayland `xdg_toplevel` has no equivalent absolute-position request; using a window rule or compositor move would test a different input. Sway clamps and centers natural floating geometry (`sway/tree/container.c:793-905,955-982`). See the adapter limitation above. |
| `135-floating-focus.t` | 82 | finished: 66 pass; 16 skip | Assertions 23–25 require distinct X11 positions that the Wayland adapter cannot request. Assertions 31, 32, 34, 35, 37, 40, 43, 46, 50, 54, 58, 62, 66, 70, 73, and 74 use i3's floating wrappers, X11-only `window` field, or opposite floating-list insertion order; equivalent direct-node checks pass where the hierarchy agrees. Layer focus modes, workspace child descent, cross-workspace focus, nested reinsertion, and close restoration match sway. Assertions 75 and 77 pass through a temporary direct-node diagnostic after preserving non-root parents across floating transitions (`sway/tree/container.c:955-1013`). Assertions 50 and 58 expect i3's new floating wrapper at index 0, while sway appends new floating containers (`sway/tree/workspace.c:961-971`). See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `136-floating-ws-empty.t` | 11 | pass | An inactive workspace remains present while it contains floating windows, matching sway's emptiness check over both tiled and non-sticky floating children (`sway/tree/workspace.c:752-764`). |
| `137-floating-unmap.t` | 2 | pass | A floating window becomes invisible after its workspace loses visibility, matching `sway/tree/view.c:1149-1172`. |
| `219-ipc-window-focus.t` | 30 | unproven | The harness refuses the file's `workspace X output fake-1` configuration instead of silently dropping it. Sway accepts workspace-to-output assignments (`sway/commands/workspace.c:145-162`), but the config translator cannot yet express one. |
| `254-move-to-output-with-criteria.t` | 16 | 14 pass; 2 fail | `fresh_workspace(output => N)` now selects each real output through sway's `focus output <direction|name>` command (`sway/commands/focus.c:310-352`). The final two assertions remain blocked by a separate parser divergence: sway accepts `move window to output fake-1 fake-2`, resolves the first name, and ignores the rest (`sway/commands/move.c:419-425,519-525`), while swayward rejects it. |
| `285-sticky.t` | 11 | unproven | The harness refuses the file's `workspace ws-on-0 output fake-0` configuration. Sway accepts this assignment (`sway/commands/workspace.c:145-162`), but the config translator cannot yet express it. |

The initial X11-protocol exclusions are `113-urgent.t`,
`162-regress-dock-urgent.t`, `196-randr-output-names.t`,
`209-ewmh-net-workarea.t`, `234-ewmh-desktop-names.t`,
`277-ipc-window-urgent.t`, `284-ewmh-visible-name.t`,
`294-update-ewmh-atoms.t`, `521-ewmh-desktop-viewport.t`, and `533-randr15.t`.

Tests that inspect X11, EWMH, RandR, XKB, or raw X events are outside this
adapter's scope. Tests requiring unsupported i3 commands such as
`append_layout` are added only when swayward implements the corresponding sway
command.
