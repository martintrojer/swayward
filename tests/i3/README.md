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
the Fedora packages.

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
| `130-close-empty-split.t` | 8 | pass | Closing an empty split restores its children to the workspace. |
| `141-resize.t` | 84 | fail (29 pass) | 21 resize commands are rejected: directional axes, attached units such as `10px`, and `px or ppt` fallbacks. The remaining failures include exact i3 geometry and floating-resize behavior. Sway supports these forms in `sway/commands/resize.c:465-550`. |
| `144-regress-floating-resize.t` | 1 | pass | Closing a floating child does not corrupt the tiled siblings' combined width. |
| `152-regress-level-up.t` | 1 | pass | `does_i3_live` after focusing above the workspace tree. |
| `178-regress-workspace-open.t` | 1 | pass | An inactive named workspace is removed after its final window closes. |
| `179-regress-multiple-ws.t` | 6 | pass | Relative `move workspace prev` resolves against sway's global workspace order before moving. |
| `191-resize-levels.t` | 3 | fail (0 pass) | `[id=…] focus` and directional resize are rejected; sway supports bare criteria focus (`sway/commands/focus.c:397-415`) and ancestor-aware directional resize (`sway/commands/resize.c:45-72`). |
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
| `129-focus-after-close.t` | 15 | fail (7 pass) | Focus-stack restoration now passes. The remaining failures concern empty-container creation, unfocused close handling, workspace kill, and floating membership; they do not inspect i3's output-level `content` node. |
| `140-focus-lost.t` | 3 | pass | Focus survives a layout change. |

The initial X11-protocol exclusions are `113-urgent.t`,
`162-regress-dock-urgent.t`, `196-randr-output-names.t`,
`209-ewmh-net-workarea.t`, `234-ewmh-desktop-names.t`,
`277-ipc-window-urgent.t`, `284-ewmh-visible-name.t`,
`294-update-ewmh-atoms.t`, `521-ewmh-desktop-viewport.t`, and `533-randr15.t`.

Tests that inspect X11, EWMH, RandR, XKB, or raw X events are outside this
adapter's scope. Tests requiring unsupported i3 commands such as
`append_layout` are added only when swayward implements the corresponding sway
command.
