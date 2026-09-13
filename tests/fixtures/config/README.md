# Sway config translator fixtures

- `sway-1.11-default.conf` is an unmodified copy of `config.in` from sway commit `793a1f0c4702e2223bded3e01623aeb5a442a3ca` (`1.11-rc2-163-g793a1f0c`).
- `swayfx-default.conf` is an unmodified copy of `config.in` from SwayFX commit `663cf66f92c2d4c99b9c2c4c79ce3538d37470ac` (`0.6-5-g663cf66f`).

`contrib/test-sway-to-kdl.py` translates both files and verifies that every manual-attention item remains in the generated output as a comment. `swayward-config/tests/translator.rs` loads each result through the config library. Replace these fixtures only with files copied from their upstream repositories.
