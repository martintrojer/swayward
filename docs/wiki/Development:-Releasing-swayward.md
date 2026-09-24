## Version and tag policy

`Cargo.toml` is the source of truth for the public version. Swayward uses
three-part Semantic Versioning. Add a hyphenated suffix for a prerelease, such
as `26.9.0-beta.1`. The version follows swayward releases. It does not claim
parity with the niri version in `docs/FORK-BASE.md`.

Tag releases as `swayward-v<version>`, for example
`swayward-v26.9.0-beta.1`. Do not use an unprefixed `v<version>` tag. The
repository retains niri tags and fetches new niri tags for every upstream
merge, so an unprefixed swayward tag can collide with an upstream tag.

The GitHub release list is the changelog. The release workflow generates the
commit list between swayward tags. Before you publish the draft, add a short
summary of user-visible changes and known limitations. Do not maintain a
second changelog file that can disagree with the releases.

## Prepare the release commit

1. Update `[workspace.package].version` in `Cargo.toml`.
2. Run `cargo check --workspace` to update the workspace package versions in
   `Cargo.lock`.
3. Update the swayward dependency pins in workspace manifests.
4. Set `pkgver` in `contrib/PKGBUILD`. For a prerelease, remove the hyphen
   and dots from its suffix (`26.9.0-beta.1` becomes `26.9.0beta1`) so Arch
   sorts it before `26.9.0`. Keep `_tagver` equal to the Cargo version.
5. Run `makepkg --printsrcinfo > .SRCINFO` from `contrib/` in an Arch
   environment.
6. Update public examples that pin the released `swayward-ipc` version.
7. Run the consistency check:

   ```sh
   ./contrib/check-release-consistency "swayward-v$(
       python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])'
   )"
   ```

The check compares the requested tag with `Cargo.toml`, `Cargo.lock`, the
workspace dependency pins, `contrib/PKGBUILD`, `contrib/.SRCINFO`, and the RPM
spec's tag-derived version.

Commit and review these changes before the final audit. Do not change the
release commit after the audit.

## Run the preflight

Run the full gate and confirm each result:

```sh
cargo test --all
cargo clippy --all --all-targets
cargo +nightly fmt --all --check
./contrib/coverage-report --check
./contrib/test-release-consistency
./contrib/sync-github-wiki --check-published
```

`coverage-report --check` must report 0 violations. Use nightly for the
formatting check; stable reports a false clean.

`sync-github-wiki --check-published` fails when the published wiki disagrees
with the repository. Run `./contrib/sync-github-wiki` to republish, then rerun
it. The release workflow runs the same check, so a stale wiki blocks the
release. Offline the command skips, so run it with network access.

Check that no wiki page still says `Since: next release`. The release workflow
fails if one does. Inherited niri pages use that phrase for options niri had
not yet released; ours say `Since: unreleased niri`, which states the same
thing without promising a release of our own.

## Cut the release

After the final audit, confirm that the release commit is the audited commit
and the working tree is clean. Creating the real beta tag is the final source
mutation:

```sh
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
./contrib/check-release-consistency "swayward-v$version"
git status --short                    # must print nothing
git tag -s "swayward-v$version" -m "swayward $version"
git push origin "swayward-v$version"
```

Do not amend the commit, move the tag, or merge another change after this
point.

Run the **Prepare release** workflow from the Actions tab. Select the tag you
just pushed as the workflow ref and enter the version without
`swayward-v`. The workflow rejects any other ref or inconsistent version. It
builds and tests on the target distributions, then drafts a GitHub release
with generated release notes and these files:

- the source tarball with vendored dependencies, for distribution builders;
- an Ubuntu 24.04 `x86_64` binary tarball and its `sha256` sum;
- an Ubuntu 24.04 `.deb`;
- one `.rpm` for each supported Fedora release, currently 43 and 44;
- an Arch Linux `.pkg.tar.zst`.

The package jobs install their artifact in a separate clean job and run both
`swayward --version` and `swaywardmsg --version`. The draft is created only
after every installation check passes.

The workflow builds on `ubuntu-24.04`, and the binary tarball targets that
distribution rather than Linux in general. The runner's glibc 2.39 becomes
the minimum version. The binary also links `libdisplay-info.so.1`, which
Ubuntu 24.04 provides. Ubuntu 22.04 and Debian 12 provide no
`libdisplay-info` package, and Debian 13 provides `.so.2`. Building on an
older runner does not fix these constraints. Wider support needs static
linking or builds for each distribution.

Fedora supports two releases at a time, so the RPM jobs run a matrix over both
and name each artifact after the release that built it. When Fedora ships a new
version, update the `fedora` matrix in the `build-rpm` and `verify-rpm` jobs and
the release list on [Getting started](./Getting-Started.md). A release that is
end of life must not stay in the matrix. The spec pins no Fedora version, so
this is a workflow change rather than a packaging change.

Review the generated release notes. Add a short summary of user-visible
changes and known limitations, then publish the draft. Do not change the tag
or its commit.

## Update external package repositories

The GitHub release already carries packages built from `swayward.spec.rpkg`,
`contrib/build-deb`, and `contrib/PKGBUILD`. COPR and AUR publication remains
a separate post-beta step:

- **Fedora**: publish `swayward.spec.rpkg` through COPR when the package has
  survived real installations.
- **Arch**: publish the checked `contrib/PKGBUILD` and `contrib/.SRCINFO` to
  the AUR. See [AUR.md](https://github.com/martintrojer/swayward/blob/main/contrib/AUR.md).
- **Nix**: the flake tracks the repository, so the tag is enough.

## Publish swayward-ipc

`swayward-ipc` is a library crate that other tools can use to speak sway's IPC
protocol:

```sh
cargo publish -p swayward-ipc
```
