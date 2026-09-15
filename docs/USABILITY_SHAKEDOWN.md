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
