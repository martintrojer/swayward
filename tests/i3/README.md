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
`open_empty_con` also creates a real
Wayland window because swayward cannot create an empty container through IPC.

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
| `178-regress-workspace-open.t` | 1 | pass | An inactive named workspace is removed after its final window closes. |
| `179-regress-multiple-ws.t` | 6 | pass | Relative `move workspace prev` resolves against sway's global workspace order before moving. |
| `191-resize-levels.t` | 3 | pass | Container split preserves the selected branch, so directional resize skips an unusable inner boundary and reaches the ancestor boundary, matching `sway/commands/resize.c:45-72`. |
| `192-layout.t` | 34 | pass | Supports default, `all`, and custom layout-toggle cycles; list forms skip unknown entries as sway does (`sway/commands/layout.c:47-95`). Sway rejects the i3 no-op `layout toggle stacked` (`layout.c:57-71`); see `docs/KNOWN_DEVIATIONS.md`. |
| `197-regression-move-vanish.t` | 2 | pass | Moving a child from a split preserves both windows. |
| `198-regression-scratchpad-crash.t` | 1 | pass | Moving and immediately showing an invisible window does not crash. |
| `204-regress-scratchpad-move.t` | 1 | pass | Moving the last window of an inactive workspace to scratchpad does not crash. |
| `224-regress-resize-branch.t` | 1 | pass | `does_i3_live` after resizing a split container. |
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
| `005-floating.t` | 13 | fail (6 pass) | The adapter now exposes window type, visibility, and tree geometry. It does not model i3's X11 `rect` creation parameter, so assertions 5 and 7–13 do not yet receive equivalent geometry input. Sway clamps and centers a view's natural size when it becomes floating (`sway/tree/container.c:793-905,955-982`). |
| `135-floating-focus.t` | 82 | fail (55 pass) | All commands parse. `focus tiling`, `focus floating`, and `focus mode_toggle` restore the target layer's active window (`sway/commands/focus.c:273-302,418-425`). Swayward now also serializes `floating_nodes` back-to-front after raising, consistent with sway's list order (`sway/tree/container.c:1682-1693`, `sway/ipc-json.c:533-539`); no captured sway fixture contains two floating windows. Assertions 31–35 still traverse i3-only wrapper children. Remaining failures cover directional floating focus, focus-child history, i3-only tree shape, and nested focus restoration. |
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
