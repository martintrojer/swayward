# Releasing swayward

## Before you tag

Run the full gate and confirm each result:

```sh
cargo test --all
cargo clippy --all --all-targets
cargo +nightly fmt --all --check
./contrib/coverage-report --check
```

`coverage-report --check` must report 0 violations. Use nightly for the
formatting check; stable reports a false clean.

Check that no wiki page still says `Since: next release`. The release workflow
fails if one does.

## Cut the release

Run the **Prepare release** workflow from the Actions tab with the version
number, without a leading `v`. It builds on `ubuntu-22.04`, runs the tests,
and drafts a GitHub release with three files:

- the source tarball with vendored dependencies, for distribution builders;
- an `x86_64` binary tarball;
- its `sha256` sum.

The workflow builds on the oldest supported runner on purpose. glibc is
forward compatible only, so the build machine's glibc becomes the minimum for
every user of the binary tarball.

Review the draft, then publish it.

## Update the packages

Each package is separate from the GitHub release:

- **Fedora**: the COPR project builds from `swayward.spec.rpkg`.
- **Arch**: bump `pkgver` in `contrib/PKGBUILD`, regenerate `.SRCINFO`, and
  push to the AUR. See [AUR.md](https://github.com/martintrojer/swayward/blob/main/contrib/AUR.md).
- **Nix**: the flake tracks the repository, so a tag is enough.

## Publish swayward-ipc

`swayward-ipc` is a library crate that other tools can use to speak sway's IPC
protocol:

```sh
cargo publish -p swayward-ipc
```
