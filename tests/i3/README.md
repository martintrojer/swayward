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

| File | Assertions | Status | Reason |
| --- | ---: | --- | --- |
| `122-split.t` | 31 | fail | Executable conformance finding. |
| `126-regress-close.t` | 1 | pass | `does_i3_live` after closing a floating container. |
| `130-close-empty-split.t` | 8 | fail | Executable conformance finding. |
| `152-regress-level-up.t` | 1 | pass | `does_i3_live` after focusing above the workspace tree. |
| `179-regress-multiple-ws.t` | 6 | pass | Relative `move workspace prev` resolves against sway's global workspace order before moving. |
| `192-layout.t` | 34 | fail | Executable conformance finding. |
| `197-regression-move-vanish.t` | 2 | pass | Moving a child from a split preserves both windows. |
| `224-regress-resize-branch.t` | 1 | pass | `does_i3_live` after resizing a split container. |
| `273-regress-focus-toggle.t` | 1 | pass | `does_i3_live` after `focus mode_toggle` on an empty workspace; sway implements this command in `sway/commands/focus.c:422`. |
| `292-regress-layout-toggle.t` | 1 | pass | `does_i3_live` after invalid `layout toggle` parameters; sway validates the accepted syntax in `sway/commands/layout.c:25-27`. |

## Coverage

| File | Assertions | Result | Notes |
| --- | ---: | --- | --- |
| `101-focus.t` | 8 | fail (3 pass) | Directional focus does not move through vertical siblings. Sway implements sibling traversal and wrapping in `sway/commands/focus.c:158-220`. |
| `104-focus-stack.t` | 2 | fail (1 pass) | Closing the focused floating window does not restore the prior tiling focus. Sway maintains inactive focus when selecting and closing views (`sway/tree/view.c:848-870`). |
| `129-focus-after-close.t` | 15 | fail (5 pass) | Parent focus, close-time focus restoration, workspace kill, and floating membership differ. Sway focuses parent nodes (`sway/commands/focus.c:355-377`) and closes all workspace descendants (`sway/commands/kill.c:20-28`). |
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
