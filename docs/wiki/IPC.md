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

Command requests return sway's array form:

```json
[{"success":false,"error":"command not implemented"}]
```

The current server implements `GET_VERSION` and `GET_TREE`. Other registered
sway message types return explicit errors while their implementations are being
completed. `GET_BAR_CONFIG` returns an empty array because swaybar configuration
is outside the current scope.

## Message types

The protocol recognises these sway request types:

- `RUN_COMMAND`
- `GET_WORKSPACES`
- `SUBSCRIBE`
- `GET_OUTPUTS`
- `GET_TREE`
- `GET_MARKS`
- `GET_BAR_CONFIG`
- `GET_VERSION`
- `GET_BINDING_MODES`
- `GET_CONFIG`
- `SEND_TICK`
- `GET_BINDING_STATE`
- `GET_INPUTS`
- `GET_SEATS`

Support for a message type means that swayward can decode it. Until its handler
is implemented, the request receives the explicit failure described above.
