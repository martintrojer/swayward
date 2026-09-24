Swayward runs upstream i3 tests against a real headless compositor, real
Wayland clients, and the sway IPC socket. The adapter replaces X11 setup but
does not edit the vendored assertions.

A passing test is evidence for the behavior it exercises, not a compatibility
percentage. Some i3 assertions cannot test sway behavior because they require
X11-only inputs, i3's parser or bar, in-place restart, or tree nodes that sway
does not expose. Those limits remain classified instead of being hidden behind
adapter substitutions.

The project keeps two kinds of evidence:

- `tests/i3/coverage.toml` records every vendored test's current TAP result.
  `contrib/coverage-report` prints the census and work queue.
- Captured sway replies and focused headless tests check IPC schemas and selected
  semantics. They do not prove every scalar value or every event variant.

For exact counts and per-assertion citations, read the
[i3 conformance report](https://github.com/martintrojer/swayward/blob/main/tests/i3/README.md).
For the mutation-tested IPC boundary, read the
[IPC oracle report](https://github.com/martintrojer/swayward/blob/main/docs/IPC_ORACLE_COVERAGE.md).
