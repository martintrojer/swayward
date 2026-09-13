# swayward-ipc

Types and helpers for interfacing with the swayward Wayland compositor, forked from [niri](https://github.com/niri-wm/niri).

## Backwards compatibility

This crate follows the swayward version.
It is **not** API-stable in terms of the Rust semver.
In particular, expect new struct fields and enum variants to be added in patch version bumps.

Use an exact version requirement to avoid breaking changes:

```toml
[dependencies]
swayward-ipc = "=26.4.0"
```
