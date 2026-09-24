The independent [sway IPC oracle](https://github.com/martintrojer/sway-ipc-oracle)
runs upstream i3 tests and captured sway IPC scenarios against i3, sway, and
swayward. The public figures come from its black-box runners, not swayward's
in-process adapter.

A passing test is evidence for the behavior it exercises, not a compatibility
percentage. Some i3 assertions cannot test sway behavior because they require
X11-only inputs, i3's parser or bar, in-place restart, or tree nodes that sway
does not expose. Those limits remain classified instead of being hidden behind
adapter substitutions.

The project keeps two kinds of evidence:

- The oracle's `i3/results/swayward.toml` and
  `sway-ipc/results/swayward.toml` record the public pass, skip, fail, match,
  mismatch, and not-applicable figures.
- `tests/i3/coverage.toml` records the faster in-process development run.
  `contrib/coverage-report` prints its census and work queue.

Run `./contrib/fetch-oracle` before local tests. The commit in
`tests/oracle.toml` pins the test files, fixtures, and black-box results.
