# IPC

swayward implements sway's i3 IPC protocol on the Unix socket named by
`SWAYSOCK`. Existing i3 and sway clients can connect without a swayward-specific
backend.

Use `swaywardmsg` to send requests:

```sh
swaywardmsg -t get_version
swaywardmsg -t get_tree
swaywardmsg -t get_workspaces
swaywardmsg -t get_outputs
```

`swaywardmsg` ships with swayward, so you do not need sway installed to drive
the socket. `swaymsg` works too if you have it.

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

See the [compatibility matrix](https://github.com/martintrojer/swayward/blob/main/docs/SWAY_COMPATIBILITY.md)
for the full request, command, and event surface. The matrix also records
partial support and deliberate deviations, including `GET_BAR_CONFIG` and its
requested-ID error. [IPC oracle coverage](https://github.com/martintrojer/swayward/blob/main/docs/IPC_ORACLE_COVERAGE.md)
records what the automated comparisons check and what they can still miss.
