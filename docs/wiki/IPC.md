# IPC

swayward implements sway's i3 IPC protocol on the Unix socket named by
`SWAYSOCK`. Existing i3 and sway clients can connect without a swayward-specific
backend.

Use `swaymsg` to send requests:

```sh
swaymsg -t get_version
swaymsg -t get_tree
swaymsg -t get_workspaces
swaymsg -t get_outputs
```

The wire format uses the standard `i3-ipc` magic header, native-endian payload
length and message type fields, and UTF-8 JSON payloads. Client libraries that
already support sway should use that protocol instead of invoking the
`swayward` binary.

## Compatibility contract

Successful replies use sway's JSON schemas. An unsupported request returns a
well-formed failure object instead of hanging or returning a private schema:

```json
{"success":false,"error":"not implemented"}
```

Command requests return sway's array form. For example, an unknown command
returns:

```json
[{"success":false,"error":"Unknown/invalid command 'frobnicate'","parse_error":true}]
```

The server implements `RUN_COMMAND`, `GET_WORKSPACES`, `SUBSCRIBE`,
`GET_OUTPUTS`, `GET_TREE`, `GET_MARKS`, `GET_BINDING_MODES`, and `GET_VERSION`.
Subscriptions support workspace, window, and mode event families.

`GET_BAR_CONFIG` returns an empty array because swayward has no `bar {}` block.
Other decoded requests without handlers return the explicit failure above.

See the [compatibility matrix](https://github.com/martintrojer/swayward/blob/main/docs/SWAY_COMPATIBILITY.md)
for request, command, and event details. The
[IPC oracle coverage](https://github.com/martintrojer/swayward/blob/main/docs/IPC_ORACLE_COVERAGE.md)
records what the automated comparisons check and what they can still miss.
