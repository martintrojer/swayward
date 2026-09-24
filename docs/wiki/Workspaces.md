swayward follows sway's workspace model. Workspaces form a global set rather
than a separate ordered list on each output.

A workspace can have a number, a name, or both. Numbered workspaces are sparse:
using workspace 10 does not create workspaces 1 through 9. A workspace is
created when a command or rule first refers to it and is removed when it is no
longer needed.

Only one workspace is visible on an output at a time. Showing a workspace on a
new output moves it from its previous output. Output assignment rules can pick
the initial output for a workspace.

Use sway-compatible runtime commands and criteria through `swaywardmsg` or
another i3 IPC client. See [IPC](./IPC.md).

## KDL declarations

The KDL configuration can declare persistent workspaces and initial output
assignments:

```kdl
workspace "1"
workspace "chat" {
    open-on-output "DP-2"
}
```

Window rules can direct matching windows to a workspace:

```kdl
window-rule {
    match app-id=r#"^org\.gnome\.Fractal$"#
    open-on-workspace "chat"
}
```

See [Named workspaces](./Configuration:-Named-Workspaces.md) for the KDL
syntax inherited by swayward.
