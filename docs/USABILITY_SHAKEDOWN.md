# Usability shakedown

- Date: 2026-09-14
- Build: `c53648a3`
- Configuration: `resources/default-config.kdl`

## Verdict

Swayward is usable for development and short evaluation sessions, but I would
not recommend it as a daily driver yet.

The core tree experience is coherent. Windows open where expected, focus and
move commands follow the tree, layouts remain understandable after several
mutations, and the compositor stayed responsive throughout the session. The
remaining problems are not cosmetic. One shipped workspace command failed,
scratchpad use produced contradictory focus metadata, and the default launch
logged a missing-component warning. Those faults affect ordinary workflows and
IPC consumers.

## What I exercised

I ran a nested compositor under a 2 GiB memory limit, with swap disabled and a
75-second wall-time limit. The run used a copy of the shipped configuration with
only `spawn-at-startup "waybar"` removed. I started a dedicated Waybar after the
nested Wayland and IPC sockets were known. The run used Waybar's stock
`sway/workspaces` module rather than the operator's full Waybar configuration,
which launches unrelated scripts.

The session covered:

- five standalone `foot` clients confirmed in the nested `GET_TREE`;
- horizontal and vertical splits;
- tabbed and stacked layouts;
- split-layout toggling;
- parent and child focus;
- directional focus and moves;
- width and height resize commands;
- numbered and named workspaces, workspace moves, rename, and back-and-forth;
- scratchpad move, show, hide, and show again;
- floating enable, absolute move, resize, and disable;
- resize binding mode entry and exit;
- Waybar 0.15.0 with the stock `sway/workspaces` module and no swayward-specific changes;
- `GET_TREE`, `GET_WORKSPACES`, and `GET_OUTPUTS` after the mutations;
- compositor and Waybar logs;
- rendered captures at initial tiling, tabbed, stacked, resized, floating, empty
  workspace, and final states.

## What works well

### The tree feels predictable

Opening four terminals produced stable, balanced tiling. Split direction,
tabbed and stacked conversion, directional moves, resize, and parent or child
focus all completed without a rejection or crash. After repeated layout changes,
the tree remained legible rather than accumulating surprising wrappers.

The commands are close enough to sway and i3 that the usual key sequence is easy
to remember. The shipped bindings cover the first-session operations without
requiring configuration work.

### The default visuals are restrained and readable

The 16-pixel gaps, 4-pixel border, clipped 12-pixel corners, and soft shadow form
a consistent visual system. The active amber border is easy to distinguish from
the dark inactive border. The 22-pixel titlebars fit terminal titles without
wasting much space.

Tabbed and stacked title strips remained readable during transitions. Focus was
usually obvious from the active border and titlebar colour. Rendering stayed
stable in the captured initial, tabbed, stacked, resized, floating, empty, and
final frames. I did not observe tearing, stale frames, misplaced windows, or
corner-radius artifacts.

This was a short nested session, not a frame-time measurement. It establishes
that the inherited animations look coherent through common tree mutations. It
does not establish performance under sustained load, multiple physical outputs,
or applications that submit frames slowly.

### Waybar starts and follows the live compositor

Waybar 0.15.0 connected without modification. Its `sway/workspaces` module
constructed successfully on the nested `winit` output and remained connected
while workspaces were created, renamed, focused, and removed. Waybar logged no
warning or error. The final `GET_WORKSPACES` response agreed with the visible
workspace state and marked only workspace 1 focused.

### IPC output is mostly useful to a person

`GET_OUTPUTS` identified the nested output and active mode clearly.
`GET_WORKSPACES` used sensible names, numbers, output names, rectangles, and
focus arrays. `GET_TREE` exposed the nested split, floating nodes, titlebar
rectangles, border style, and border width in a form that was easy to inspect.

## What feels unfinished

### First launch assumes optional software is installed

The only compositor warning was:

```text
error spawning xwayland-satellite at "xwayland-satellite", disabling integration:
No such file or directory (os error 2)
```

The compositor continued normally, but a new user sees a warning on the default
path before doing anything. The experience needs either a clearer packaging
contract or quieter handling when this optional component is absent. Task
`m6_default_config_xwayland_warning` records the finding.

### Advanced workflows still need command knowledge

The default bindings make common tree operations accessible, but scratchpad,
floating placement, criteria, and named-workspace workflows remain hard to
discover from the startup overlay alone. This is acceptable for an i3-oriented
compositor, but the first ten minutes still depend on knowing sway commands.

### The visual conclusion is provisional

Common animations looked smooth in this run, but the programme still lacks an
automated animation oracle. The check did not cover multiple physical refresh
rates, fractional-scale movement, slow clients, shader failures, or long-lived
GPU pressure. Visual quality is promising, not proven.

## What is broken

### `workspace back_and_forth` depends on trailing whitespace

After visiting workspaces 1 and 2, the exact command
`workspace back_and_forth` returned:

```json
{"success":false,"error":"No workspace was previously active."}
```

The same command with trailing spaces succeeded. This is not the documented
workspace-ordering deviation. It is a parser or dispatch bug in an ordinary sway
command. Task `m6_fix_workspace_back_and_forth_trailing_space` contains the
reproduction.

### Scratchpad cycling leaves contradictory focus fields

After moving a window to scratchpad, showing it, hiding it, and showing it again,
`GET_TREE` reported both the tiled leaf and the floating scratchpad leaf as
`focused: true` on workspace 1. The workspace focus array pointed only to the
tiled leaf. An inactive workspace also retained a leaf with `focused: true`.

A client cannot determine authoritative focus from that payload. Task
`m6_fix_scratchpad_focus_ambiguity` records the sequence and the conflicting
fields.

## Daily-driver assessment

Not yet.

For someone developing swayward or testing it in a nested session, the current
build is useful and substantially better than a prototype. The tree, rendering,
titlebars, workspace widget, and ordinary commands form a credible desktop.

For daily use, the workspace command failure and contradictory focus metadata
are blockers. The first breaks muscle memory. The second can mislead bars,
automation, and debugging tools. After those defects are fixed, the next useful
trial should run for several hours on physical outputs with real applications,
fractional scaling, suspend and resume, and a normal Waybar configuration.

## Verification

`cargo test --all` reported 384 passing tests and no failures:

- `swayward`: 344
- `swayward-config`: 23
- `swayward-config/tests/translator.rs`: 9
- `swayward-config/tests/wiki-parses.rs`: 1
- `swayward-ipc`: 6
- documentation tests: 1

# Second usability shakedown

- Date: 2026-09-15
- Build: `c902a1d1`
- Configuration: `resources/default-config.kdl`, with `spawn-at-startup "waybar"`
  removed for controlled startup

## Verdict

Swayward is ready for short development sessions, but this run does not justify
a daily-driver recommendation.

The three blockers from the first shakedown no longer reproduce. Workspace
back-and-forth recreates a reaped workspace, scratchpad cycling leaves one
focused node, and a missing xwayland-satellite logs at `INFO`. The tree and IPC
were stable with three real Foot clients. Two parts of the requested trial were
not proved: virtual-keyboard input did not trigger live bindings, and the nested
winit backend exposed only one output. Pointer events synthesized by the outer
Sway session also produced no observable nested event, so mouse behavior needs a
physical-input rerun before it can support a usability verdict.

## Method and limits

I ran each nested compositor in a single shell under
`MemoryMax=2G`, `MemorySwapMax=0`, `WLR_RENDERER=pixman`, and `timeout 58`.
The shell launched clients, queried the explicit nested IPC socket, captured
screenshots, and reaped clients and sockets on exit. Before querying the outer
compositor, it unset the nested `SWAYSOCK` and passed the saved outer socket
explicitly.

The three terminals were standalone `foot` processes. Foot 1.27 rejects
`--server-socket=/dev/null`; that option belongs to `footclient`. Standalone
Foot has no server handoff. I discarded an initial run that used the invalid
option and continued only after `GET_TREE` contained all three expected titles.

The nested winit backend supplied one 1905×2112 output. Swayward has no live IPC
command to add a second output, and its multi-output headless backend is exposed
through the Rust fixture rather than the executable. I did not substitute a
unit test for the requested hands-on two-output trial. Task
`m6_live_two_output_trial` records this limitation.

I used `wtype` and a temporary `/dev/uinput` device to send keyboard input. Both
methods exited successfully, but neither caused a configured release binding,
a binding-mode transition, or a default layout binding. The same behaviors pass
through the in-process input backend, so this run could not compare their live
feel. Task `m6_virtual_keyboard_live_bindings` records the reproduction. This
finding was reproduced before the final rebase; the final build changed only the
i3 harness and test inventory, not the live input path.

I sent pointer motion, button, drag, and wheel input through the outer Sway seat
to coordinates inside the nested window. The outer commands succeeded, but
nested focus, marks, and floating geometry did not change. This result might be
an outer synthetic-input limitation rather than a swayward defect. Task
`m6_nested_pointer_delivery` requests a physical-pointer check.

## Confirmed improvements

### Workspace back-and-forth survives reaping

I opened three terminals, switched to workspace 2, and moved all three windows
there so workspace 1 disappeared from `GET_WORKSPACES`. The exact command
`workspace back_and_forth` returned success and recreated focused workspace 1.
No trailing whitespace was required. This fixes the first shakedown's ordinary
workflow blocker.

### Scratchpad focus has one source of truth

I moved a focused terminal to the scratchpad, showed it, hid it, and showed it
again. Each of the four `GET_TREE` captures contained exactly one node with
`focused: true`. The shown captures focused the floating scratchpad node. The
hidden captures focused the remaining tiled node. I found no stale focused leaf
on an inactive workspace.

This verifies the previous scratchpad fix through a real client and repeated
show/hide commands, not only its unit test.

### Optional X11 support is quiet

Each valid launch logged:

```text
INFO ... xwayland-satellite not found at "xwayland-satellite"; X11 integration disabled
```

The compositor log contained no warning or error during the valid default-config
run on the final build. The missing optional component no longer presents as a
startup failure.

## What worked

Three Foot clients opened in balanced horizontal tiles and remained mapped while
the compositor responded to IPC operations. The initial geometry had 16-pixel outer and inner gaps. The
shipped configuration requests a 4-pixel border, while each leaf reported
`current_border_width: 2` and a 22-pixel titlebar. The amber active edge remained
easy to distinguish from inactive edges at this large nested size. The gaps felt
deliberate rather than wasteful. The border setting and IPC value need an oracle
check before treating either number as authoritative.

`resize set width 500 px` and `resize set width 60 ppt` both returned success and
changed live `GET_TREE` geometry. The pixel form produced a 491-pixel focused
leaf after borders. The percentage form produced a 1123-pixel focused leaf in a
1873-pixel usable width. It also reduced one adjacent leaf to 104 pixels while a
third remained 614 pixels. That result was visually harsh enough to warrant an
oracle check; task `m6_resize_set_ppt_geometry` records it without calling it a
bug yet.

Marks and criteria worked when driven over IPC. `mark trialmark` appeared in
`GET_MARKS`, and `[con_mark=trialmark] move left` returned success without losing
the mark.

Waybar 0.15.0 connected with a stock `sway/workspaces` module. It configured a
bar for output `winit`, stayed connected across workspace changes, and logged no
warning or error before the capped compositor shutdown.

A real GTK transient child opened while its parent was fullscreen under
`popup-during-fullscreen "leave_fullscreen"`. `GET_TREE` showed the parent with
`fullscreen_mode: 0` and the child as a floating node. This confirms that the
policy leaves fullscreen on a live xdg-toplevel parent relationship.

The final rendered captures showed intact borders, titlebars, shadows, and gaps.
I saw no stale frame or obvious clipping defect. As in the first run, this is a
short nested software-rendered session. Visual quality is promising, not proven.

## What remains broken or unproven

- Live virtual-keyboard events did not exercise default, release, or mode
  bindings. This blocks an honest hands-on assessment of those interactions.
- The live executable supplied one nested output. Scratchpad behavior across two
  outputs remains unproven.
- Synthetic outer-seat pointer events did not produce observable input in the
  nested compositor. Titlebar clicks, border clicks, drag, and title-strip
  scrolling remain unproven until repeated with physical pointer input.
- The failed live binding path prevented a reliable startup-overlay audit. I do
  not claim that the overlay rendered or that every visible label matches its
  binding. A static source comparison is weaker than the requested user check.
- The default bindings use Alt as `Mod` under nested winit and Super on a TTY.
  The command set is familiar to an i3 or sway user, but that backend-dependent
  modifier makes a nested evaluation less representative of a real session.

## Daily-driver assessment

Not yet, based on the evidence available from this run.

The previous back-and-forth, scratchpad-focus, and startup-warning blockers are
fixed in real use. Ordinary IPC-driven tree, workspace, scratchpad, mark,
criteria, resize, popup, and Waybar workflows were stable. The remaining concern
is confidence rather than a reproduced crash or data-loss bug: live keyboard
bindings, physical pointer behavior, and two-output scratchpad use were not
proved by this environment. A daily-driver verdict needs those checks on a TTY
session with two physical outputs, followed by a longer run with suspend and
resume, fractional scaling, and normal applications.

## Verification for the second shakedown

After rebasing onto `c902a1d1`, the repository gates passed:

- `swayward`: 421 tests
- `swayward-config`: 30 unit tests
- `swayward-config/tests/translator.rs`: 12 tests
- `swayward-config/tests/wiki-parses.rs`: 1 test
- `swayward-ipc`: 7 tests
- documentation tests: 1
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo +nightly fmt --all --check`
- `python3 contrib/test-sway-to-kdl.py`: 46 tests

All 225 vendored i3 files were byte-identical to the reference checkout. The
green manifest contained 98 files, and the working tree had no snapshot churn.
