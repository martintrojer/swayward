# DRAFT: beta1 release notes

These notes are a draft for maintainer review. Beta1 is the line in the sand:
after its tag is published, swayward stops rewriting history and starts merging
niri release tags. See [Upstream maintenance](UPSTREAM.md) for the long version.

Most changes below came from comparing swayward with sway rather than from a
sudden outbreak of taste. If an old result was useful to you, each entry says
how to keep it when that is possible.

## Changes you may notice on the desktop

### Tiled windows have no gaps by default

The default inner and outer gaps changed from 16 pixels to zero. This matches
sway's initialized gap values (`sway/config.c:288-291`).

To keep the old spacing, add this to your KDL configuration:

```kdl
layout {
    gaps 16
}
```

### Titlebar font sizes use Pango points

A titlebar font such as `monospace 10` is now measured and rendered as a
10-point Pango font. Older swayward builds converted that value to absolute
pixels. At scale 1, the default titlebar is therefore 27 pixels high rather
than 22 pixels high. Sway passes its configured Pango font description through
to titlebar measurement and uses the resulting height when it arranges
containers (`sway/config.c`; `sway/tree/container.c`; `sway/tree/arrange.c`).

To restore the old unit, specify pixels in the Pango font description:

```kdl
layout {
    titlebar {
        font "monospace 10px"
    }
}
```

### Sticky floating windows no longer take focus after a workspace switch

When you switch workspaces, sticky floating windows still follow you. They no
longer replace the destination workspace's existing focus. An empty destination
keeps workspace focus, and an occupied destination keeps its focused window.
Sway focuses the destination before it reparents sticky containers
(`sway/input/seat.c:1178-1222`).

There is no setting that restores the old focus-stealing behavior. A binding or
IPC script can focus the sticky window after the workspace command if that is
the workflow you want.

### Output removal keeps the evacuated workspace focused

When an output disappears, its active workspace moves to a surviving output
and remains active. When the output returns, workspace priority can move that
workspace back. Sway performs the same evacuation and priority restoration
(`sway/tree/output.c:31-45,158-159,205-247`).

An empty workspace left behind during evacuation is now removed after focus
moves away from it. This matches sway's empty-workspace destruction
(`sway/tree/workspace.c:313-330`) and prevents an inactive workspace that
cannot be addressed later.

There is no compatibility setting for the former focus or empty-workspace
lifecycle. Create or select a named persistent workspace after hotplug if you
need one to remain.

### Empty workspaces start with a fresh split

After the final tiled window leaves a workspace, later selecting that workspace
name starts with the default split instead of reviving its old tabbed or stacked
root. Sway destroys the empty workspace and creates a new tree when the name is
selected again (`sway/tree/workspace.c:274-332`).

There is no setting that retains the old layout on an empty workspace. Set the
layout again after selecting it, or keep a window on the workspace.

### A directional move sets the axis even with one tiled window

On a workspace with one tiled window, `move left` or `move right` now leaves a
horizontal split axis, while `move up` or `move down` leaves a vertical one.
Sway applies the directional move's axis before it tries to move the container
past a sibling (`sway/commands/move.c`; `sway/tree/container.c`). This matters
when the next window opens.

There is no switch for the old early return. Run `split h` or `split v` after
the move to choose the axis explicitly.

### Headless output commands change the real output

On the headless backend, configured output modes now resize the compositor and
appear in `GET_OUTPUTS`. Disabling an output now evacuates its workspaces and
reports the connector as inactive; enabling it reconnects the output. Sway
routes output configuration through output enable, disable, mode, and evacuation
paths before serializing it (`sway/config/output.c:241-294,703-738`;
`sway/tree/output.c:31-56`; `sway/ipc-json.c:299-413`).

There is no setting for the former config-only behavior. Scripts that used a
successful command without expecting the output to change must stop issuing
that command.

## Command reply changes

Commands that need a focused container now fail on an empty workspace instead
of succeeding as no-ops. This includes the affected `move`, `floating`,
`scratchpad`, `resize`, `mark`, and `sticky` forms. Sway dispatches these
commands against the focused workspace node and returns either `CMD_INVALID` or
`CMD_FAILURE` from the command handler (`sway/commands/move.c`;
`sway/commands/floating.c`; `sway/commands/resize.c`;
`sway/commands/scratchpad.c`; `sway/commands/mark.c`;
`sway/commands/sticky.c`).

The JSON reply now also follows sway's classification: invalid commands set
`parse_error` to `true`, while ordinary command failures set it to `false`
(`sway/commands.c`; `sway/ipc-json.c`).

There is no compatibility setting. If a script intended a no-op, check that a
container is focused before sending the command, or accept the structured
failure.

## IPC tree and workspace changes

The IPC serializer now follows sway more closely. This is good news for clients
and mildly inconvenient news for snapshots made from older swayward builds.

- Split containers report `floating: "auto_off"` and
  `scratchpad_state: "none"`, as sway's common container serializer does
  (`sway/ipc-json.c:710-744`).
- `border none` keeps the stored `current_border_width` even though no border is
  drawn (`sway/commands/border.c:44-51`; `sway/ipc-json.c:543-555`).
- Tabbed and stacked children report only the active child as visible. Each tab
  or stack child reports the full arranged parent share, and nested titlebar
  rows are included in the geometry (`sway/tree/view.c:1180-1193`;
  `sway/tree/arrange.c:185-215`; `sway/ipc-json.c:746-755`).
- Ordinary split percentages come from rounded arranged extents, including gap
  space, rather than only from stored split weights (`sway/ipc-json.c:746-755`).
- Output percentages use each output's horizontal share of the root. Two
  equal-width outputs now have equal percentages even when their heights differ
  (`sway/ipc-json.c`).
- Workspace rectangles include their effective inner and outer gap insets
  (`sway/tree/workspace.c`; `sway/tree/arrange.c`).
- Fullscreen trees now report sway's focus, visibility, border, decoration, and
  percentage state. Hidden siblings keep their pre-fullscreen shares
  (`sway/tree/view.c:698-710`; `sway/ipc-json.c:710-761`).
- A workspace that has never held a tiled window has a null `representation`.
  After its tiled root has held a window, an emptied vertical root retains
  `V[]`, including when every remaining window is floating or in the scratchpad
  (`sway/tree/container.c`; `sway/ipc-json.c:523-524`).
- Nested `layout tabbed` and `layout stacking` commands preserve the existing
  grouped workspace root instead of replacing the wrong level
  (`sway/commands/layout.c`; `sway/tree/container.c`).
- Output and workspace focus arrays retain the complete most-recently-used
  order instead of only the active and previous workspace
  (`sway/tree/root.c:246-260`; `sway/ipc-server.c:604-610,825-834`).
- Tiled windows now expose their stored sticky state in `GET_TREE`, like
  floating windows (`sway/commands/sticky.c`; `sway/ipc-json.c:710-744`).

There is no old-wire-format mode. IPC clients should consume the sway-shaped
values and avoid comparing entire replies with snapshots from pre-beta1 builds.
If a client needs historical behavior, transform the JSON in that client rather
than asking the compositor to send two protocols under one name.

## The oracle moved out

The unchanged i3 tests, sway captures, calibration tools, and black-box runners
now live in
[`sway-ipc-oracle`](https://github.com/martintrojer/sway-ipc-oracle). Swayward
pins commit `3d8159e7dbc1b296d52b00e45c12befcdcae605b` in
[`tests/oracle.toml`](../tests/oracle.toml), and CI fetches that exact revision.
Run `./contrib/fetch-oracle` before the in-process conformance tests and coverage
tools.

This move does not change compositor behavior. It makes the evidence harder to
edit by accident: swayward keeps its adapter and coverage ledger, while the
tests it is measured against live in a separate repository at a recorded SHA.
To use another oracle revision for local investigation, edit `tests/oracle.toml`
and fetch again. Do not commit that change unless the oracle update is the work
being reviewed.
