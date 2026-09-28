# swayward-ipc

Types and blocking clients for interfacing with the swayward Wayland compositor.

Use `sway_socket::SwaySocket` for sway-compatible IPC over `$SWAYSOCK`. `MessageType` selects the
request, `send` returns its raw JSON reply, and `read_event` reads events after a subscribe request.
The `legacy` and `socket` modules use swayward's separate line-delimited protocol over
`$SWAYWARD_SOCKET`; new sway-compatible clients should not use that endpoint.

```rust,no_run
use swayward_ipc::{sway_socket::SwaySocket, MessageType};

let mut socket = SwaySocket::connect()?;
let tree = socket.send(MessageType::GetTree, "")?;
println!("{tree}");
# Ok::<(), swayward_ipc::sway_socket::SwayError>(())
```

## Backwards compatibility

This crate follows the swayward version.
It is **not** API-stable in terms of the Rust semver.
In particular, expect new struct fields and enum variants to be added in patch version bumps.

Use an exact version requirement to avoid breaking changes:

```toml
[dependencies]
swayward-ipc = "=26.4.0"
```
