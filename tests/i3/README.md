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
| `115-ipc-workspaces.t` | 8 | 8 fail | Workspace creation emits one generic `reload` event instead of sway's `init`, `focus`, and `empty` sequence. Sway emits those events at `sway/tree/workspace.c:268`, its focus path, and `sway/tree/workspace.c:301`; their payloads use `sway/sway/ipc-server.c:295-320`. |
| `117-workspace.t` | 92 | finished: 67 pass; 21 fail; 4 skip | A temporary direct-workspace diagnostic reached all assertions. Assertion 28 exposes a negative-number serialization bug; 20 assertions expose the unsupported `rename workspace` command, which sway implements in `sway/commands/rename.c:15-105`. Assertions 57, 63, 69, and 75 traverse i3's output-level `content` node and are skipped as i3-only tree structure (`sway/sway/ipc-json.c:869-874`); the equivalent direct-node assertion 51 passes. |
| `176-workspace-baf.t` | 26 | unproven | The file launches two compositor configurations, changes `workspace_auto_back_and_forth`, and restarts i3. The headless adapter runs one fixed compositor and has no launch, configuration-reload, or restart lifecycle, so it cannot ask these assertions without changing their inputs. |
| `178-regress-workspace-open.t` | 1 | pass | An inactive named workspace is removed after its final window closes. |
| `179-regress-multiple-ws.t` | 6 | pass | Relative `move workspace prev` resolves against sway's global workspace order before moving. |
| `191-resize-levels.t` | 3 | pass | Container split preserves the selected branch, so directional resize skips an unusable inner boundary and reaches the ancestor boundary, matching `sway/commands/resize.c:45-72`. |
| `192-layout.t` | 34 | pass | Supports default, `all`, and custom layout-toggle cycles; list forms skip unknown entries as sway does (`sway/commands/layout.c:47-95`). Sway rejects the i3 no-op `layout toggle stacked` (`layout.c:57-71`); see `docs/KNOWN_DEVIATIONS.md`. |
| `197-regression-move-vanish.t` | 2 | pass | Moving a child from a split preserves both windows. |
| `198-regression-scratchpad-crash.t` | 1 | pass | Moving and immediately showing an invisible window does not crash. |
| `204-regress-scratchpad-move.t` | 1 | pass | Moving the last window of an inactive workspace to scratchpad does not crash. |
| `224-regress-resize-branch.t` | 1 | pass | `does_i3_live` after resizing a split container. |
| `227-ipc-workspace-empty.t` | 8 | 4 pass; 4 fail | Closing the final window on an invisible workspace emits sway's `empty` event with the correct workspace. Workspace switches emit generic `reload` rather than sway's `focus`, and omit `empty` when the old workspace is destroyed (`sway/tree/workspace.c:301`; `sway/sway/ipc-server.c:295-320`). |
| `273-regress-focus-toggle.t` | 1 | pass | `does_i3_live` after `focus mode_toggle` on an empty workspace; sway implements this command in `sway/commands/focus.c:422`. |
| `292-regress-layout-toggle.t` | 1 | pass | `does_i3_live` after invalid `layout toggle` parameters; sway validates the accepted syntax in `sway/commands/layout.c:25-27`. |
| `299-regress-scratchpad-focus.t` | 1 | pass | Showing a scratchpad window from another workspace moves and focuses it, matching `sway/tree/root.c:157-200`. |
| `303-regress-move-floating.t` | 3 | pass | Moving a nested floating container leaves two tiled nodes and no floating node. |

## Coverage

| File | Assertions | Result | Notes |
| --- | ---: | --- | --- |
| `101-focus.t` | 8 | pass | Directional focus follows sway's sibling traversal, ancestor escalation, and wrapping rules (`sway/commands/focus.c:158-220`). |
| `104-focus-stack.t` | 2 | pass | Closing the focused floating window restores the prior tiling focus, matching sway's focus-stack restoration (`sway/input/seat.c:260-315`). |
| `129-focus-after-close.t` | 15 | finished: 13 pass; 2 skip | Assertions 5 and 6 require i3's `open` command and empty containers. Sway has no `open` entry in its command tables (`sway/sway/commands.c:44-144`), so swayward correctly rejects it. The adapter substitutes a real Wayland window for `open_empty_con`; passing assertions that use this helper do not prove empty-container behavior. See [The i3 `open` command and empty containers](../../docs/KNOWN_DEVIATIONS.md#the-i3-open-command-and-empty-containers). |
| `140-focus-lost.t` | 3 | pass | Focus survives a layout change. |
| `005-floating.t` | 13 | finished: 6 pass; 7 unproven | Assertions 5 and 7–13 depend on i3's X11 `rect` creation input. Wayland `xdg_toplevel` has no equivalent absolute-position request; using a window rule or compositor move would test a different input. Sway clamps and centers natural floating geometry (`sway/tree/container.c:793-905,955-982`). See the adapter limitation above. |
| `135-floating-focus.t` | 82 | finished: 66 pass; 16 skip | Assertions 23–25 require distinct X11 positions that the Wayland adapter cannot request. Assertions 31, 32, 34, 35, 37, 40, 43, 46, 50, 54, 58, 62, 66, 70, 73, and 74 use i3's floating wrappers, X11-only `window` field, or opposite floating-list insertion order; equivalent direct-node checks pass where the hierarchy agrees. Layer focus modes, workspace child descent, cross-workspace focus, nested reinsertion, and close restoration match sway. Assertions 75 and 77 pass through a temporary direct-node diagnostic after preserving non-root parents across floating transitions (`sway/tree/container.c:955-1013`). Assertions 50 and 58 expect i3's new floating wrapper at index 0, while sway appends new floating containers (`sway/tree/workspace.c:961-971`). See [Floating container wrappers](../../docs/KNOWN_DEVIATIONS.md#floating-container-wrappers). |
| `136-floating-ws-empty.t` | 11 | pass | An inactive workspace remains present while it contains floating windows, matching sway's emptiness check over both tiled and non-sticky floating children (`sway/tree/workspace.c:752-764`). |
| `137-floating-unmap.t` | 2 | pass | A floating window becomes invisible after its workspace loses visibility, matching `sway/tree/view.c:1149-1172`. |

The initial X11-protocol exclusions are `113-urgent.t`,
`162-regress-dock-urgent.t`, `196-randr-output-names.t`,
`209-ewmh-net-workarea.t`, `234-ewmh-desktop-names.t`,
`277-ipc-window-urgent.t`, `284-ewmh-visible-name.t`,
`294-update-ewmh-atoms.t`, `521-ewmh-desktop-viewport.t`, and `533-randr15.t`.

Tests that inspect X11, EWMH, RandR, XKB, or raw X events are outside this
adapter's scope. Tests requiring unsupported i3 commands such as
`append_layout` are added only when swayward implements the corresponding sway
command.
