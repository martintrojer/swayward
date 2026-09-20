# Key bindings

Key bindings are declared in the `binds` section of
`~/.config/swayward/config.kdl`.

```kdl
binds {
    Mod+H { command "focus left"; }
    Mod+1 { command "workspace 1"; }
    Mod+Return { command "exec alacritty"; }
    Super+Alt+L { spawn "swaylock"; }
}
```

A hotkey consists of modifiers separated by `+`, followed by an XKB key name.
Valid modifiers include `Ctrl`, `Shift`, `Alt`, `Super`, `Mod3`, `Mod5`, and
`Mod`. `Mod` defaults to `Super` in a full session and `Alt` in a nested window.
Use `wev` to find XKB key names.

Use `command "..."` to run a sway command. The command must be one quoted KDL
string. Swayward stores the string when loading the config, then parses and
executes it when the key is pressed, as sway does.

The inherited typed actions remain available for compositor functions that the
current sway command subset cannot express, including screenshots and session
control.

## Bind properties

Set `repeat=false` to prevent a held key from repeating. Set `cooldown-ms` to
limit how often a repeating bind runs.

```kdl
binds {
    Mod+T repeat=false { spawn "alacritty"; }
    XF86AudioRaiseVolume cooldown-ms=50 {
        spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "0.02+"
    }
}
```

Set `allow-when-locked=true` only on a `spawn` bind that must work while the
session is locked. Set `allow-inhibiting=false` on an escape bind that must work
while an application inhibits shortcuts.

Set `input-device` to limit a binding to one input device. Its value uses sway's
`vendor:product:name` identifier, with spaces in the device name replaced by
underscores. The default `"*"` matches every device.

```kdl
binds {
    x input-device="1234:5678:Example_Keyboard" { command "nop"; }
}
```

## Pointer bindings

Mouse buttons and wheel directions can appear in bindings:

```kdl
binds {
    Mod+MouseLeft { close-window; }
    Mod+WheelScrollDown { spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "0.02-"; }
}
```

A pointer bind acts on the window focused when the event occurs.

## `spawn`

`spawn` executes a program directly. Write each argument as a separate KDL
string. It does not expand shell variables, pipelines, or `~` in arguments.

```kdl
binds {
    Mod+Return { spawn "alacritty" "-e" "/usr/bin/fish"; }
}
```

### `spawn-sh`

Use `spawn-sh` when shell syntax is required:

```kdl
binds {
    Mod+D { spawn-sh "fuzzel | sh"; }
}
```

## Common actions

### `toggle-window-rule-opacity`

This action toggles the opacity supplied by the matching window rule.

### Custom hotkey overlay titles

Set `hotkey-overlay-title` on a bind to control its label in the important
hotkeys overlay. Set it to `null` to hide that bind.

### Other actions

These inherited actions are useful outside the tree command language:

- `quit` exits after confirmation. Add `skip-confirmation=true` to bypass it.
- `screenshot`, `screenshot-screen`, and `screenshot-window` capture the screen.
- `toggle-window-floating` moves a window between the tree and floating layer.
- `toggle-windowed-fullscreen` changes the application's fullscreen state
  without covering the output.
- `toggle-keyboard-shortcuts-inhibit` escapes an application's shortcut
  inhibitor.

Run `swaywardmsg -t command '<command>'` for runtime tree and workspace commands.
See [IPC](./IPC.md).
