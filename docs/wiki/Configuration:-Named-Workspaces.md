swayward uses sway's global workspace model. A workspace name may begin with a
number, such as `1` or `2:web`; that number is exposed as the workspace's `num`
field over IPC. Names without a numeric prefix have `num` set to `-1`.

Declare a persistent workspace at the top level of the KDL config:

```kdl
workspace "1"
workspace "chat" {
    open-on-output "DP-2"
}
```

`open-on-output` assigns the workspace to an output when it is created. The
output can be a connector name or its manufacturer, model, and serial string.

Window rules can select a workspace by name:

```kdl
window-rule {
    match app-id=r#"^org\.gnome\.Fractal$"#
    open-on-workspace "chat"
}
```

Runtime workspace commands use sway's command language over `SWAYSOCK`. See
[Workspaces](./Workspaces.md) and [IPC](./IPC.md).
