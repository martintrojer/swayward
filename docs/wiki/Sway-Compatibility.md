Swayward speaks sway's IPC protocol on `SWAYSOCK`. Existing tools such as
`swaymsg` and Waybar can connect without a swayward-specific backend.

Queries, workspace and window commands, and the event families used by common
clients are the strongest part of the interface. Unsupported requests and
commands return explicit failures. Swayward does not return plausible-looking
private data in a sway response.

The main limits are:

- `RUN_COMMAND` does not yet cover every runtime command that sway accepts.
- IPC schemas target sway 1.12. The fixture suite pins the exact source tag and
  detects field-set drift. `GET_OUTPUTS` reports live transform, geometry,
  scale, identity, and mode, but not backend adaptive-sync, tearing, HDR,
  render-time, or power state.
- `GET_CONFIG` returns a not-implemented response because sway's reply contains
  sway config text, while swayward uses KDL.
- Bar configuration and bar events are unavailable because swayward does not
  manage a bar. Configure Waybar directly.

Use [IPC](IPC.md) for commands and examples. Read [Differences from
sway](Differences-from-Sway.md) before migrating an existing session.

For exact request, event, and command rows, open the
[compatibility reference](https://github.com/martintrojer/swayward/blob/main/docs/SWAY_COMPATIBILITY.md).
The [IPC oracle report](https://github.com/martintrojer/swayward/blob/main/docs/IPC_ORACLE_COVERAGE.md)
records the measured test boundary.
