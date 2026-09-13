# Known deviations from sway

This reference lists user-visible differences from sway. It does not list
internal changes inherited from the niri fork. See `docs/DIVERGENCE.md` for that
engineering ledger.

## Configuration

Swayward does not parse sway configuration files. It uses typed KDL configuration
to retain niri's nested effect and animation settings. Use the
[sway config migration guide](SWAY_CONFIG_MIGRATION.md) to translate an existing
configuration.

Bindings can contain sway command strings:

```kdl
binds {
    Mod+H { command "focus left"; }
}
```

Swayward also retains typed niri actions for features outside the current sway
command subset.

## Binding modes

Swayward has no binding modes. This affects common configurations because sway's
own default config defines a `resize` mode. The translator reports both the mode
block and bindings that enter it for manual attention.

`GET_BINDING_MODES` returns a well-formed not-implemented error. Mode event
subscriptions are accepted, but normal operation cannot produce a mode change.

## Bars

Swayward has no `bar {}` configuration block and does not launch swaybar.
`GET_BAR_CONFIG` returns an empty array.

Use waybar as an external layer-shell client. Waybar 0.15.0 was tested manually
with its `sway/workspaces`, `sway/window`, and `sway/mode` modules. The test used
a nested compositor and is not part of the automated suite.

## Titlebars and tabs

Swayward does not render server-side titlebars. `deco_rect` is therefore empty,
including on bordered windows. Sway reports the real titlebar rectangle there.

Tabbed and stacked containers retain their layout and keyboard behavior, but they
do not display window-title labels. The inherited `TabIndicator` is a configurable
accent, not a sway titlebar.

## IPC requests and commands

`GET_BINDING_MODES`, `GET_CONFIG`, `GET_INPUTS`, and `GET_SEATS` return:

```json
{"success":false,"error":"not implemented"}
```

`SEND_TICK` and `GET_BINDING_STATE` currently return the same error. Commands
outside the implemented subset return a sway-shaped `RUN_COMMAND` failure. See
the [compatibility matrix](SWAY_COMPATIBILITY.md) for the supported subset and
the [IPC oracle coverage](IPC_ORACLE_COVERAGE.md) for its test boundary.

## Layout and Xwayland

Swayward has no scrollable-tiling mode. It uses an i3-style nested container tree.
Niri's horizontal viewport and overview animations were retired because their
layout no longer exists.

X11 applications run through `xwayland-satellite`. Swayward does not include
sway's in-process Xwayland window manager.
