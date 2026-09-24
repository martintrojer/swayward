# Publishing swayward to the AUR

`contrib/PKGBUILD` is the package source. It is built and linted before every
release, but publishing needs an AUR account, which is a manual step.

## Build and lint it

No Arch machine required; a container is enough.

```sh
distrobox create --name arch-pkg --image docker.io/library/archlinux:latest --yes
distrobox enter arch-pkg -- bash -lc 'sudo pacman -Sy --needed --noconfirm base-devel namcap'

cd contrib
distrobox enter arch-pkg -- bash -lc 'cd contrib && makepkg -s'
distrobox enter arch-pkg -- bash -lc 'cd contrib && namcap PKGBUILD *.pkg.tar.zst'
```

`namcap` must report no `E:` lines. The `W:` lines about implicitly satisfied
dependencies are expected: they name transitive libraries pulled in by
`cairo`, `pango` and `mesa`.

## Two things that will bite you

**LTO breaks the build.** Arch enables `lto` in `makepkg.conf` by default.
`libspa-sys` generates a C shim for SPA's static inline functions, and LTO
discards those objects before the Rust side links them:

```
rust-lld: error: undefined symbol: spa_format_parse_libspa_rs
```

`options=('!lto')` in the PKGBUILD handles it. Do not remove it.

**PipeWire lives in `libpipewire`, not `pipewire`.** The `pipewire` package is
the daemon; `libpipewire` carries `libspa-0.2.pc` and the shared library the
build needs.

## Publish

Requires an account on https://aur.archlinux.org with an SSH key registered
in your profile, and this in `~/.ssh/config`:

```
Host aur.archlinux.org
  IdentityFile ~/.ssh/aur
  User aur
```

Then:

```sh
git clone ssh://aur@aur.archlinux.org/swayward.git aur-swayward
cd aur-swayward
cp ../contrib/PKGBUILD .
makepkg --printsrcinfo > .SRCINFO      # AUR rejects a push without this
git add PKGBUILD .SRCINFO
git commit -m "swayward 26.4.0-1"
git push
```

The first push creates the package page. Verify the connection first with
`ssh aur@aur.archlinux.org help`, which prints your AUR username on success.

## Updating

Bump `pkgver`, regenerate `.SRCINFO`, commit both, push. The AUR does not
show a new version until `.SRCINFO` is regenerated.
