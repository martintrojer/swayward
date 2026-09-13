# Build swayward

The development host runs Fedora Sway Atomic. Its system image is immutable, so build swayward in a dedicated Distrobox instead of layering development packages onto the host.

## Create the container

Run the setup script from the repository root:

```sh
./contrib/dev-container.sh
```

The script creates `swayward-dev` from `registry.fedoraproject.org/fedora-toolbox:44` and installs the build dependencies. You can run it again to update an existing container. The test dependencies include Perl, `Test::More`, and `JSON::PP`; the i3 conformance runner executes upstream Perl test files against swayward's headless compositor and IPC socket.

## Build and test

Distrobox mounts the host home directory at the same path. Run Cargo in the repository path inside the container:

```sh
distrobox enter swayward-dev -- bash -lc 'cd /var/home/martintrojer/hacking/swayward && cargo build'
distrobox enter swayward-dev -- bash -lc 'cd /var/home/martintrojer/hacking/swayward && cargo test --all'
```

The baseline at commit `1f0ab41f` (fork base `9e72e491`) is:

- `cargo build`: exit 0.
- `cargo test --all`: exit 0, with 218 passed, 0 failed, and 0 ignored across all test binaries and doc tests.

Later changes can match this baseline but must not regress from it.

## Run a nested session

Distrobox passes `WAYLAND_DISPLAY` and the host Wayland socket into the container. On this source revision, the nested winit backend is selected automatically when `WAYLAND_DISPLAY` is set:

```sh
distrobox enter swayward-dev -- bash -lc 'cd /var/home/martintrojer/hacking/swayward && cargo run'
```

The process opened a nested compositor window on the Fedora Sway host and ran until interrupted. The proposed `cargo run -- --backend winit` command does not apply to this revision: the binary rejects `--backend` with exit 2.
