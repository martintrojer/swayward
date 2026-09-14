#!/usr/bin/env python3
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).parents[1]
SCRIPT = Path(__file__).with_name("sway-to-kdl")


class TranslatorTests(unittest.TestCase):
    def translate(self, source: str):
        with tempfile.TemporaryDirectory() as directory:
            source_path = Path(directory) / "config"
            source_path.write_text(source)
            return subprocess.run(
                [SCRIPT, source_path], text=True, capture_output=True, check=True
            )

    def test_translates_supported_directives_and_reports_the_rest(self):
        result = self.translate(
            """
set $mod Mod4
set $term foot
bindsym --locked $mod+Return exec $term
bindcode 24 kill
input type:touchpad {
    tap enabled
    natural_scroll disabled
}
input type:keyboard {
    xkb_layout us
    repeat_delay 300
}
output DP-1 resolution 1920x1080 position 10,20 scale 1.5
for_window [app_id="firefox"] floating enable
assign [class="^mail$"] → 2: mail
bar {
    position top
}
blur enable
corner_radius 8
shadows enable
dim_inactive 0.5
layer_effects "waybar" blur enable
mystery value
bindsym $missing+x nop
"""
        )
        self.assertIn('Super+Return allow-when-locked=true { command "exec foot"; }', result.stdout)
        self.assertIn('code:24 { command "kill"; }', result.stdout)
        self.assertIn("touchpad {", result.stdout)
        self.assertIn("tap", result.stdout)
        self.assertIn("natural-scroll false", result.stdout)
        self.assertIn('layout "us"', result.stdout)
        self.assertIn("repeat-delay 300", result.stdout)
        self.assertNotIn("$mod", result.stdout)
        self.assertNotIn("$term", result.stdout)
        self.assertIn('output "DP-1" {', result.stdout)
        self.assertIn('mode "1920x1080"', result.stdout)
        self.assertIn("position x=10 y=20", result.stdout)
        self.assertIn("scale 1.5", result.stdout)
        self.assertIn('match app-id="firefox"', result.stdout)
        self.assertIn("open-floating true", result.stdout)
        self.assertIn('match app-id="^mail$"', result.stdout)
        self.assertIn('open-on-workspace "2: mail"', result.stdout)
        self.assertIn("bar blocks are unsupported; use waybar", result.stdout)
        self.assertIn("SwayFX blur -> swayward blur", result.stdout)
        self.assertIn("SwayFX corner_radius -> window-rule geometry-corner-radius", result.stdout)
        self.assertIn("SwayFX shadows; tune remaining shadow controls", result.stdout)
        self.assertIn("SwayFX layer_effects -> layer-rule", result.stdout)
        self.assertIn('match namespace="^waybar$"', result.stdout)
        self.assertIn("blur true", result.stdout)
        self.assertIn("dim_inactive -> unfocused window opacity", result.stdout)
        self.assertIn("opacity 0.5", result.stdout)
        self.assertIn("unhandled: mystery value", result.stdout)
        self.assertIn("undefined variable", result.stdout)
        self.assertIn("manual attention:", result.stderr)
        for item in result.stderr.splitlines()[1:]:
            self.assertIn(item.strip(), result.stdout)

    def test_assign_workspace_target_is_preserved(self):
        result = self.translate('assign [class="special"] workspace targetws\n')
        self.assertIn('open-on-workspace "targetws"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_assign_output_target_uses_open_on_output(self):
        result = self.translate('assign [class="special"] output DP-1\n')
        self.assertIn('open-on-output "DP-1"', result.stdout)
        self.assertNotIn("open-on-workspace", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_assign_workspace_number_requires_manual_conversion(self):
        result = self.translate('assign [class="special"] workspace number 2\n')
        self.assertNotIn("open-on-workspace", result.stdout)
        self.assertIn("workspace-number assignments are unsupported", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_assign_rejects_invalid_workspace_number(self):
        result = self.translate('assign [class="special"] workspace number nope\n')
        self.assertNotIn("open-on-workspace", result.stdout)
        self.assertIn("invalid workspace number 'nope'", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_x11_class_translates_to_wayland_app_id_without_treating_regex_anchors_as_variables(self):
        result = self.translate(
            'for_window [class="^special$"] floating enable, floating disable\n'
        )
        self.assertIn('match app-id="^special$"', result.stdout)
        self.assertNotIn("open-floating true", result.stdout)
        self.assertIn("open-floating false", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_duplicate_bind_is_reported_instead_of_silently_overwritten(self):
        result = self.translate("bindsym Mod4+h focus left\nbindsym Mod4+h focus right\n")
        self.assertIn('Super+h { command "focus left"; }', result.stdout)
        self.assertNotIn('command "focus right"', result.stdout)
        self.assertIn("duplicate binding for Super+h; kept the first", result.stdout)

    def test_bindcode_is_preserved_as_a_numeric_trigger(self):
        result = self.translate("bindcode --no-repeat 24 kill\n")
        self.assertIn('code:24 repeat=false { command "kill"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_checked_in_sway_and_swayfx_defaults_keep_manual_items(self):
        fixtures = ROOT / "tests" / "fixtures" / "config"
        for source in [
            fixtures / "sway-1.11-default.conf",
            fixtures / "swayfx-default.conf",
        ]:
            with self.subTest(source=source.name):
                result = subprocess.run(
                    [SCRIPT, source], text=True, capture_output=True, check=True
                )
                self.assertIn("manual attention:", result.stderr)
                for item in result.stderr.splitlines()[1:]:
                    self.assertIn(item.strip(), result.stdout)

    def test_include_is_recursive_and_cycles_are_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "child").write_text("bindsym Mod4+h focus left\ninclude config\n")
            (root / "config").write_text("include child\n")
            result = subprocess.run(
                [SCRIPT, root / "config"], text=True, capture_output=True, check=True
            )
        self.assertIn('Super+h { command "focus left"; }', result.stdout)
        self.assertIn("include cycle", result.stdout)
        self.assertIn("manual attention:", result.stderr)


if __name__ == "__main__":
    unittest.main()
