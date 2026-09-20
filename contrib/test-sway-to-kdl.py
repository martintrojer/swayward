#!/usr/bin/env python3
import json
import os
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

    def test_sway_ignored_client_directives_remain_visible_with_exact_reason(self):
        result = self.translate(
            "client.background #111111 #222222 #333333 #444444 #555555\n"
            "client.placeholder anything sway does not parse\n"
        )
        for directive in ["client.background", "client.placeholder"]:
            self.assertIn(
                f"{directive} is accepted but ignored by sway itself "
                "(sway/sway/commands.c:51,55; sway/sway/commands/client.c:79-81)",
                result.stdout,
            )
            self.assertNotIn(f"unhandled: {directive}", result.stdout)
        self.assertIn("manual attention: 2 directive(s)", result.stderr)
    def test_xkb_numlock_uses_sways_boolean_vocabulary_outside_xkb(self):
        for value, expected in [
            ("1", "numlock"),
            ("yes", "numlock"),
            ("on", "numlock"),
            ("true", "numlock"),
            ("enable", "numlock"),
            ("enabled", "numlock"),
            ('"enabled"', "numlock"),
            ("active", "numlock"),
            ("toggle", "numlock"),
            ("0", "numlock false"),
            ("no", "numlock false"),
            ("off", "numlock false"),
            ("false", "numlock false"),
            ("disable", "numlock false"),
            ("disabled", "numlock false"),
            ("inactive", "numlock false"),
            ("garbage", "numlock false"),
        ]:
            with self.subTest(value=value):
                result = self.translate(
                    f"input type:keyboard {{\n    xkb_numlock {value}\n}}\n"
                )
                self.assertIn(f"        {expected}\n", result.stdout)
                self.assertNotIn("xkb {\n            numlock", result.stdout)
                self.assertIn(
                    f"// sway-to-kdl: sway xkb_numlock {value}", result.stdout
                )
                self.assertIn("manual attention: none", result.stderr)

    def test_xkb_numlock_under_device_selector_stays_fail_loud(self):
        result = self.translate(
            "input 1234:5678:keyboard {\n    xkb_numlock enabled\n}\n"
        )
        self.assertNotIn("        numlock\n", result.stdout)
        self.assertIn(
            "device-specific input selectors require manual conversion", result.stdout
        )
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_force_display_urgency_hint_translates_to_timeout(self):
        for directive in [
            "force_display_urgency_hint 300",
            "force_display_urgency_hint 500 ms",
            "force_display_urgency_hint 700ms",
            "force_display_urgency_hint -1ms",
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                expected = 0 if directive.startswith("force_display_urgency_hint -") else int(
                    directive.split()[1].removesuffix("ms")
                )
                self.assertIn(f"urgent-timeout-ms {expected}", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_titlebar_padding_maps_one_and_two_argument_forms(self):
        for directive, horizontal, vertical in [
            ("titlebar_padding 6", 6, 6),
            ("titlebar_padding 6 3", 6, 3),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertIn(f"// sway-to-kdl: SwayFX {directive}", result.stdout)
                self.assertIn(f"horizontal-padding {horizontal}", result.stdout)
                self.assertIn(f"vertical-padding {vertical}", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_titlebar_padding_rejects_negative_and_invalid_values(self):
        for directive in [
            "titlebar_padding 0",
            "titlebar_padding -1",
            "titlebar_padding 6 -1",
            "titlebar_padding nope",
            "titlebar_padding 65536",
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertNotIn("horizontal-padding", result.stdout)
                self.assertNotIn("vertical-padding", result.stdout)
                reason = (
                    "padding below SwayFX default titlebar_border_thickness 1 cannot be preserved"
                    if directive == "titlebar_padding 0"
                    else "Invalid size specified"
                )
                self.assertIn(reason, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_top_level_exec_preserves_shell_commands_and_ignores_startup_id_flag(self):
        for directive, command in [
            ("exec waybar", "waybar"),
            ("exec --no-startup-id mako", "mako"),
            ('exec "notify-send hello world"', "notify-send hello world"),
            ("exec sh -c 'printf one,two; printf three'", "sh -c 'printf one,two; printf three'"),
            ("exec swaymsg workspace 2", "swaymsg workspace 2"),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertIn(f"spawn-sh-at-startup {json.dumps(command)}", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_exec_always_preserves_startup_effect_and_warns_about_reload(self):
        result = self.translate("exec_always --no-startup-id session-start --reload\n")
        self.assertIn('spawn-sh-at-startup "session-start --reload"', result.stdout)
        self.assertIn("exec_always reload behavior is not preserved", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_top_level_exec_does_not_change_bind_exec_translation(self):
        result = self.translate("exec waybar\nbindsym Mod4+x exec fuzzel --show drun\n")
        self.assertIn('spawn-sh-at-startup "waybar"', result.stdout)
        self.assertIn('Super+x { command "exec fuzzel --show drun"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_empty_bind_command_fails_loud(self):
        result = self.translate("bindsym X\n")
        self.assertNotIn('X { command', result.stdout)
        self.assertIn("malformed bindsym", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_explicit_default_mode_is_preserved(self):
        result = self.translate('mode "default" {\n    bindsym X nop\n}\n')
        self.assertIn('mode "default" {', result.stdout)
        self.assertIn('X { command "nop"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_nested_variable_names_are_accepted_without_recursive_expansion(self):
        result = self.translate(
            "set $long_variable_name_with_short_value 1\n"
            "set $$long_variable_name_with_short_value 2\n"
            "set $$$long_variable_name_with_short_value 3\n"
        )
        self.assertIn("manual attention: none", result.stderr)

        result = self.translate("set $a $b\nset $b expanded\nbindsym X nop $a\n")
        self.assertIn("variable value could not be fully expanded", result.stdout)
        self.assertIn("undefined variable", result.stdout)
        self.assertIn("manual attention: 2 directive(s)", result.stderr)

        result = self.translate("set $x expanded\nbindsym X nop $$x\n")
        self.assertIn('X { command "nop $x"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_bare_resize_command_is_preserved_for_typed_validation(self):
        result = self.translate('mode "default" {\n    bindsym X resize\n}\n')
        self.assertIn('X { command "resize"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_focus_wrapping_maps_all_modes_case_insensitively_and_refuses_garbage(self):
        for value in ["yes", "NO", "Force", "wOrKsPaCe"]:
            with self.subTest(value=value):
                result = self.translate(f"focus_wrapping {value}\n")
                self.assertIn(f'focus-wrapping "{value.lower()}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate("focus_wrapping sideways\n")
        self.assertNotIn("focus-wrapping", result.stdout)
        self.assertIn(
            "unknown focus wrapping mode is refused instead of treated as no",
            result.stdout,
        )
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_workspace_layout_maps_all_sway_values_and_refuses_invalid_values(self):
        for value in ["default", "stacking", "tabbed"]:
            with self.subTest(value=value):
                result = self.translate(f"workspace_layout {value}\n")
                self.assertIn(f'workspace-layout "{value}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)
        result = self.translate("workspace_layout splitv\n")
        self.assertNotIn("workspace-layout", result.stdout)
        self.assertIn("unsupported workspace layout", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_default_orientation_maps_all_sway_values(self):
        for value in ["horizontal", "vertical", "auto"]:
            with self.subTest(value=value):
                result = self.translate(f"default_orientation {value}\n")
                self.assertIn(f'default-orientation "{value}"', result.stdout)
                self.assertIn(f"// sway-to-kdl: sway default_orientation {value}", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate("default_orientation diagonal\n")
        self.assertNotIn("    default-orientation", result.stdout)
        self.assertIn("expected default_orientation horizontal|vertical|auto", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_mouse_button_bindsyms_map_exact_names_in_top_level_and_modes(self):
        result = self.translate(
            "bindsym button1 nop left\n"
            "bindsym button2 nop middle\n"
            "bindsym button3 nop right\n"
            "bindsym button4 nop up\n"
            "bindsym button5 nop down\n"
            "mode test {\n"
            "    bindsym button1 nop mode-left\n"
            "}\n"
        )
        for trigger, command in [
            ("MouseLeft", "nop left"),
            ("MouseMiddle", "nop middle"),
            ("MouseRight", "nop right"),
            ("WheelScrollUp", "nop up"),
            ("WheelScrollDown", "nop down"),
            ("MouseLeft", "nop mode-left"),
        ]:
            self.assertIn(f'{trigger} {{ command "{command}"; }}', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        result = self.translate("bindsym button10 nop unsupported\n")
        self.assertNotIn("command", result.stdout)
        self.assertIn("unsupported mouse button button10", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_sway_modifier_names_map_exactly_and_unknown_names_stay_fail_loud(self):
        result = self.translate(
            "bindsym Shift+a nop shift\n"
            "bindsym Lock+b nop caps\n"
            "bindsym Control+c nop control\n"
            "bindsym Ctrl+d nop ctrl\n"
            "bindsym Mod1+e nop mod1\n"
            "bindsym Alt+f nop alt\n"
            "bindsym Mod2+g nop mod2\n"
            "bindsym Mod4+i nop mod4\n"
            "bindsym Super+j nop super\n"
        )
        for key in [
            "Shift+a",
            "Lock+b",
            "Control+c",
            "Ctrl+d",
            "Alt+e",
            "Alt+f",
            "Num+g",
            "Super+i",
            "Super+j",
        ]:
            self.assertIn(f"    {key} ", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        for name in ["Mod3", "Mod5"]:
            refused = self.translate(f"bindsym {name}+z nop x\n")
            self.assertNotIn("command", refused.stdout)
            self.assertIn(f"unsupported modifier {name}", refused.stdout)

        result = self.translate("bindsym Mod6+a nop unsupported\n")
        self.assertNotIn("command", result.stdout)
        self.assertIn("unsupported modifier Mod6", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_floating_modifier_translates_normal_and_refuses_inexact_forms(self):
        for directive, expected in [
            ("floating_modifier Mod4", 'mod-key "Super"'),
            ("floating_modifier mod4 NORMAL", 'mod-key "Super"'),
            ("floating_modifier MOD1 normal", 'mod-key "Alt"'),
            # Sway checks only argv[1], so a third word is accepted.
            ("floating_modifier Mod4 normal extra", 'mod-key "Super"'),
            ("floating_modifier none", 'mod-key "None"'),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(f"{directive}\n")
                self.assertIn(expected, result.stdout)
                self.assertNotIn("mod-key-nested", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        for directive, reason in [
            (
                "floating_modifier Mod4 Inverse",
                "inverse move/resize buttons have no exact swayward equivalent",
            ),
            ("floating_modifier Mod6", "unsupported modifier Mod6"),
            (
                "floating_modifier Mod4 sideways",
                "expected floating_modifier <mod> [inverse|normal]",
            ),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(f"{directive}\n")
                self.assertNotIn("mod-key ", result.stdout)
                self.assertIn(reason, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_gesture_and_switch_bindings_fail_loud_with_specific_reasons(self):
        gesture_reason = (
            "gesture events cannot be bound to sway commands; "
            "only built-in compositor gestures and client forwarding are available"
        )
        switch_reason = (
            "switch-events cannot preserve sway command bindings, binding modes, "
            "toggle, --locked, or --reload semantics"
        )
        for directive in [
            "bindgesture swipe workspace next",
            "bindgesture --exact swipe:3:left workspace prev",
            'bindgesture --input-device="type:touchpad" pinch:4:inward exec lock',
            "unbindgesture swipe:3:left",
        ]:
            with self.subTest(directive=directive):
                result = self.translate(f"{directive}\n")
                self.assertIn(gesture_reason, result.stdout)
                self.assertNotIn("unhandled:", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

        for directive in [
            "bindswitch lid:off exec lock",
            "bindswitch tablet:toggle exec rotate",
            "bindswitch --locked --no-warn --reload lid:on exec wake",
            "unbindswitch tablet:off",
        ]:
            with self.subTest(directive=directive):
                result = self.translate(f"{directive}\n")
                self.assertIn(switch_reason, result.stdout)
                self.assertNotIn("unhandled:", result.stdout)
                self.assertNotIn("switch-events {", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

        result = self.translate(
            'mode "resize" {\n'
            "    bindgesture hold:2 exec menu\n"
            "    bindswitch --reload lid:off exec lock\n"
            "}\n"
        )
        self.assertIn(gesture_reason, result.stdout)
        self.assertIn(switch_reason, result.stdout)
        self.assertNotIn("unhandled directive in mode", result.stdout)
        self.assertIn("manual attention: 2 directive(s)", result.stderr)

    def test_release_bindings_translate_for_keys_mouse_and_modes(self):
        result = self.translate(
            "bindsym Control+Print nop key-press\n"
            "bindsym --release Control+Print nop key-release\n"
            "bindsym button1 --release nop mouse-release\n"
            "mode test {\n"
            "    bindcode 27 --release nop mode-release\n"
            "}\n"
        )
        for binding in [
            'Control+Print release=true repeat=false { command "nop key-release"; }',
            'MouseLeft release=true repeat=false { command "nop mouse-release"; }',
            'code:27 release=true repeat=false { command "nop mode-release"; }',
        ]:
            self.assertIn(binding, result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_mouse_binding_region_before_key_translates(self):
        result = self.translate("bindsym --whole-window button4 nop button4\n")
        self.assertIn(
            'WheelScrollUp mouse-regions="titlebar+border+contents" { command "nop button4"; }',
            result.stdout,
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_mouse_binding_regions_after_key_keep_translating(self):
        for option, regions in [
            ("--whole-window", "titlebar+border+contents"),
            ("--border", "border"),
            ("--exclude-titlebar", "border+contents"),
        ]:
            with self.subTest(option=option):
                result = self.translate(f"bindsym button1 {option} nop supported\n")
                self.assertIn(
                    f'MouseLeft mouse-regions="{regions}" {{ command "nop supported"; }}',
                    result.stdout,
                )
                self.assertIn("manual attention: none", result.stderr)

    def test_mouse_binding_region_options_combine_like_sway(self):
        for options, regions in [
            ("--border --border", "border"),
            ("--border --whole-window", "titlebar+border+contents"),
            ("--whole-window --exclude-titlebar", "border+contents"),
        ]:
            with self.subTest(options=options):
                result = self.translate(f"bindsym {options} button1 nop supported\n")
                self.assertIn(
                    f'MouseLeft mouse-regions="{regions}" {{ command "nop supported"; }}',
                    result.stdout,
                )
                self.assertIn("manual attention: none", result.stderr)

    def test_unsupported_leading_bind_options_remain_fail_loud(self):
        for option in ["--inhibited", "--to-code"]:
            with self.subTest(option=option):
                result = self.translate(f"bindsym {option} a nop unsupported\n")
                self.assertNotIn('{ command "nop unsupported"; }', result.stdout)
                self.assertIn(f"unsupported bindsym option {option}", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_numeric_bindsym_is_quoted_at_top_level_and_in_modes(self):
        result = self.translate(
            "bindsym 1 workspace number 1\n"
            "mode resize {\n"
            "    bindsym 2 workspace number 2\n"
            "}\n"
            "bindcode nope kill\n"
        )
        self.assertEqual(
            result.stdout.count('"1" { command "workspace number 1"; }'), 1
        )
        self.assertEqual(
            result.stdout.count('"2" { command "workspace number 2"; }'), 1
        )
        self.assertIn('mode "resize" {', result.stdout)
        self.assertIn("bindcode key must be numeric", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_inner_gaps_accept_sway_units_and_clamp_negative_values(self):
        for value, expected in [("10", "10"), ("20px", "20"), ("14PX", "14"), ("-5px", "0")]:
            with self.subTest(value=value):
                result = self.translate(f"gaps inner {value}\n")
                self.assertIn(f"    gaps {expected}", result.stdout)
                self.assertNotIn("px", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_outer_gap_forms_translate_without_using_struts(self):
        expected = {
            "outer": ["left", "right", "top", "bottom"],
            "horizontal": ["left", "right"],
            "vertical": ["top", "bottom"],
            "top": ["top"],
            "right": ["right"],
            "bottom": ["bottom"],
            "left": ["left"],
        }
        for kind, sides in expected.items():
            with self.subTest(kind=kind):
                result = self.translate(f"gaps {kind} -10px\n")
                self.assertNotIn("struts {", result.stdout)
                for side in sides:
                    self.assertIn(f"        {side} -10", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_workspace_gap_overrides_translate_to_workspace_layout(self):
        result = self.translate(
            "gaps outer 22\n"
            "workspace 1 gaps outer 0\n"
            "workspace 1 gaps inner 0\n"
            "workspace 4 gaps left 10\n"
            "workspace 4 gaps top 20\n"
        )
        self.assertIn('workspace "1" {\n    layout {', result.stdout)
        self.assertIn('workspace "4" {\n    layout {', result.stdout)
        self.assertIn("            left 10", result.stdout)
        self.assertIn("            top 20", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_default_valued_unsupported_directives_are_reported_as_satisfied(self):
        for directive in ["swaybg_command -", "smart_gaps off", "smart_gaps OFF"]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertIn(f"// sway-to-kdl: satisfied by swayward defaults: {directive}", result.stdout)
                self.assertNotIn("unhandled", result.stdout)
                self.assertNotIn("dynamic workspace-box", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_non_default_forms_remain_fail_loud(self):
        for directive, reason in [
            ("swaybg_command mybg", "custom swaybg launch command is unsupported"),
            ("smart_gaps on", "smart_gaps requires dynamic workspace-box recomputation"),
            ("smart_gaps inverse_outer", "smart_gaps requires dynamic workspace-box recomputation"),
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertIn(reason, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_gap_forms_refuse_unrepresentable_or_malformed_values(self):
        for source in [
            "gaps inner nope\n",
            "gaps outer 2em\n",
            "gaps diagonal 10\n",
            "gaps outer all set 10px\n",
        ]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_top_level_nop_is_accepted_and_ignored(self):
        result = self.translate("nop comment text\n")
        self.assertIn("sway nop comment text", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_nop_preserves_an_effect_free_rule(self):
        for command in ["nop", "nop anything is ignored"]:
            with self.subTest(command=command):
                result = self.translate(f'for_window [app_id="^demo$"] {command}\n')
                self.assertIn(
                    'window-rule {\n'
                    '    match app-id="^demo$"\n'
                    f'    sway-for-window-command "{command}"\n'
                    '}',
                    result.stdout,
                )
                self.assertIn("manual attention: none", result.stderr)

    def test_for_window_nop_combines_with_real_effects(self):
        result = self.translate(
            'for_window [app_id="^demo$"] border none; nop comment text\n'
        )
        self.assertIn(
            'window-rule {\n'
            '    match app-id="^demo$"\n'
            '    sway-border "none"\n'
            '    sway-for-window-command "nop comment text"\n'
            '}',
            result.stdout,
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_nop_prefix_is_refused(self):
        result = self.translate('for_window [app_id="^demo$"] nopper\n')
        self.assertNotIn("sway-for-window-command", result.stdout)
        self.assertIn("command needs manual conversion", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_title_format_preserves_wayland_placeholders(self):
        result = self.translate(
            'for_window [app_id="^demo$"] title_format [%app_id] %title\n'
        )
        self.assertIn(
            'sway-for-window-command "title_format [%app_id] %title"', result.stdout
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_title_format_preserves_all_sway_placeholders(self):
        placeholders = " ".join(
            [
                "%title",
                "%app_id",
                "%class",
                "%instance",
                "%shell",
                "%sandbox_engine",
                "%sandbox_app_id",
                "%sandbox_instance_id",
            ]
        )
        result = self.translate(
            f'for_window [app_id="^demo$"] title_format {placeholders}\n'
        )
        self.assertIn(
            f'sway-for-window-command "title_format {placeholders}"', result.stdout
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_bind_input_device_strips_quotes_and_preserves_spaces_and_colons(self):
        result = self.translate(
            'bindsym --input-device="123:456:keyboard with spaces" x nop exact\n'
        )
        self.assertIn(
            'x input-device="123:456:keyboard with spaces" { command "nop exact"; }',
            result.stdout,
        )
        self.assertIn("manual attention: none", result.stderr)

    def test_bind_groups_are_preserved_for_symbols_and_codes(self):
        result = self.translate(
            "bindsym Group2+x nop symbol\n"
            "bindsym Mode_switch+y nop alias\n"
            "bindcode Group3+42 nop code\n"
        )
        self.assertIn('Group2+x { command "nop symbol"; }', result.stdout)
        self.assertIn('Mode_switch+y { command "nop alias"; }', result.stdout)
        self.assertIn('Group3+code:42 { command "nop code"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_assign_workspace_target_is_preserved(self):
        result = self.translate('assign [class="special"] workspace targetws\n')
        self.assertIn('open-on-workspace "targetws"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_assign_output_target_uses_open_on_output(self):
        result = self.translate('assign [class="special"] output DP-1\n')
        self.assertIn('open-on-output "DP-1"', result.stdout)
        self.assertNotIn("open-on-workspace", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_assign_workspace_number_has_a_distinct_target(self):
        result = self.translate(
            'assign [class="numbered"] workspace number 2\n'
            'assign [class="short"] number 2\n'
            'assign [class="named"] 2\n'
        )
        self.assertEqual(result.stdout.count('open-on-workspace-number "2"'), 2)
        self.assertEqual(result.stdout.count('open-on-workspace "2"'), 1)
        self.assertIn("manual attention: none", result.stderr)

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

    def test_deprecated_default_borders_translate_without_changing_shipped_defaults(self):
        result = self.translate("new_window 1pixel\nnew_float normal 3\n")
        self.assertIn("// sway new_window 1pixel", result.stdout)
        self.assertIn('sway-border "pixel"', result.stdout)
        self.assertIn("sway-border-width 1", result.stdout)
        self.assertIn('sway-floating-border "normal"', result.stdout)
        self.assertIn("sway-floating-border-width 3", result.stdout)
        self.assertIn("manual attention: none", result.stderr)
        self.assertNotIn("resources/default-config.kdl", result.stdout)

    def test_hide_edge_borders_and_smart_borders_are_independent(self):
        expected = {
            "none": ('hide-edge-borders "none"', None),
            "vertical": ('hide-edge-borders "vertical"', None),
            "horizontal": ('hide-edge-borders "horizontal"', None),
            "both": ('hide-edge-borders "both"', None),
            "smart": ('hide-edge-borders "none"', 'smart-borders "on"'),
            "smart_no_gaps": ('hide-edge-borders "none"', 'smart-borders "no-gaps"'),
        }
        for value, (edge, smart) in expected.items():
            with self.subTest(value=value):
                result = self.translate(f"hide_edge_borders {value}\n")
                self.assertIn(edge, result.stdout)
                if smart:
                    self.assertIn(smart, result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate("hide_edge_borders smart\nsmart_borders off\n")
        self.assertIn('hide-edge-borders "none"', result.stdout)
        self.assertIn('smart-borders "off"', result.stdout)
        self.assertNotIn('smart-borders "on"', result.stdout)

        for source in ["hide_edge_borders --i3 smart\n", "hide_edge_borders sideways\n"]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

        for source, expected in [
            ("smart_borders sometimes\n", "off"),
            ("smart_borders on\nsmart_borders toggle\n", "off"),
            ("smart_borders off\nsmart_borders toggle\n", "on"),
        ]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertIn(f'smart-borders "{expected}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_default_border_forms_and_invalid_values(self):
        for directive in ["default_border", "default_floating_border", "new_window", "new_float"]:
            for value in ["none", "normal", "pixel", "1pixel", "pixel 5", "normal 7"]:
                with self.subTest(directive=directive, value=value):
                    result = self.translate(f"{directive} {value}\n")
                    self.assertIn("manual attention: none", result.stderr)
        for source in [
            "default_border csd\n",
            "default_floating_border 2pixel 3\n",
            "new_window pixel nope\n",
            "new_float none 2\n",
            "new_window pixel 65536\n",
        ]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertIn("unsupported default border", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_criteria_accept_bare_and_quoted_values(self):
        result = self.translate(
            'for_window [urgent=latest] focus\n'
            'for_window [class="quoted"] border none\n'
            'for_window [title="quoted value"] border none\n'
        )
        self.assertIn('match is-urgent=true', result.stdout)
        self.assertIn('match app-id="quoted"', result.stdout)
        self.assertIn('match title="quoted value"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        unterminated = self.translate('for_window [title="unterminated] border none\n')
        self.assertNotIn("window-rule {", unterminated.stdout)
        self.assertIn("criteria need manual conversion", unterminated.stdout)
        self.assertIn("manual attention: 1 directive(s)", unterminated.stderr)

    def test_for_window_combines_class_and_title_without_losing_regex_escapes(self):
        result = self.translate(
            r'''for_window [class="^foo\\w+$" title="^bar\\d+$"] border none
'''
        )
        self.assertIn(r'match app-id="^foo\\\\w+$" title="^bar\\\\d+$"', result.stdout)
        self.assertIn('sway-border "none"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_translates_sway_pixel_border_forms(self):
        for command, width in [("border pixel", None), ("border pixel 2", 2)]:
            with self.subTest(command=command):
                result = self.translate(f'for_window [class="foo"] {command}\n')
                self.assertIn('sway-border "pixel"', result.stdout)
                if width is None:
                    self.assertNotIn("sway-border-width", result.stdout)
                else:
                    self.assertIn(f"sway-border-width {width}", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_for_window_pixel_border_combines_in_both_orders_and_separators(self):
        for separator in [",", ";"]:
            for command in [
                f"border pixel 2{separator} floating enable",
                f"floating enable{separator} border pixel 2",
            ]:
                with self.subTest(command=command):
                    result = self.translate(f'for_window [class="foo"] {command}\n')
                    self.assertIn('sway-border "pixel"', result.stdout)
                    self.assertIn("sway-border-width 2", result.stdout)
                    self.assertIn("open-floating true", result.stdout)
                    self.assertIn("manual attention: none", result.stderr)

    def test_for_window_rejects_invalid_pixel_width_with_sway_syntax(self):
        expected = (
            "Expected 'border <none|normal|pixel|csd|toggle>' "
            "or 'border pixel <px>'"
        )
        for width in ["-2", "nope"]:
            with self.subTest(width=width):
                result = self.translate(f'for_window [class="foo"] border pixel {width}\n')
                self.assertNotIn('sway-border "pixel"', result.stdout)
                self.assertIn(expected, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_keeps_supported_effects_around_an_unsupported_part(self):
        result = self.translate(
            'for_window [class="foo"] border pixel 2, exec nope; floating enable\n'
        )
        self.assertIn('sway-border "pixel"', result.stdout)
        self.assertIn("sway-border-width 2", result.stdout)
        self.assertIn("open-floating true", result.stdout)
        self.assertNotIn('sway-for-window-command "exec nope"', result.stdout)
        self.assertIn("command needs manual conversion", result.stdout)
        self.assertIn("for_window [class=\"foo\"] exec nope", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_translates_floating_size_and_center_shape(self):
        result = self.translate(
            'for_window [class="foo"] floating enable, '
            'resize set 50 ppt 60 ppt, move position center\n'
        )
        self.assertIn("open-floating true", result.stdout)
        self.assertIn("default-column-width { proportion 0.5; }", result.stdout)
        self.assertIn("default-window-height { proportion 0.6; }", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_preserves_sticky_as_a_map_time_command(self):
        for value in ["enable", "disable", "toggle"]:
            with self.subTest(value=value):
                result = self.translate(
                    f'for_window [title="^pip$"] floating enable, sticky {value}, '
                    "border pixel 2\n"
                )
                self.assertIn("open-floating true", result.stdout)
                self.assertIn(f'sway-for-window-command "sticky {value}"', result.stdout)
                self.assertIn('sway-border "pixel"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

    def test_for_window_criteria_accept_inline_regex_with_bracket_class(self):
        result = self.translate(
            'for_window [title="(?i)^picture[- ]in[- ]picture$"] '
            "floating enable, sticky enable, border pixel 2\n"
        )
        self.assertIn('match title="(?i)^picture[- ]in[- ]picture$"', result.stdout)
        self.assertIn('sway-for-window-command "sticky enable"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_line_continuation_precedes_variable_expansion(self):
        result = self.translate(
            "set \\\n$var \\\nspecial title\n"
            'for_window \\\n[title="$var"] \\\nborder \\\nnone\n'
        )
        self.assertIn('match title="special title"', result.stdout)
        self.assertIn('sway-border "none"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_overlong_valid_and_invalid_continued_bindings_are_not_truncated(self):
        payload = "x" * 5000
        valid = self.translate(f"bindsym X nop \\\n{payload}\n")
        self.assertIn(f'command "nop  {payload}"', valid.stdout)
        self.assertIn("manual attention: none", valid.stderr)

        invalid = self.translate(f"bindsym X invalid-{payload[:8]} \\\n{payload}\n")
        self.assertIn(f'command "invalid-{payload[:8]}  {payload}"', invalid.stdout)
        self.assertIn("manual attention: none", invalid.stderr)

    def test_last_line_without_newline_is_translated(self):
        result = self.translate("set $ws workspace eggs\nbindsym Mod4+0 $ws")
        self.assertIn('Super+0 { command "workspace eggs"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_popup_during_fullscreen_maps_all_modes_case_insensitively(self):
        for value, expected in [
            ("smart", "smart"),
            ("IGNORE", "ignore"),
            ("Leave_Fullscreen", "leave_fullscreen"),
        ]:
            with self.subTest(value=value):
                result = self.translate(f"popup_during_fullscreen {value};\n")
                self.assertIn(f'popup-during-fullscreen "{expected}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        for directive in [
            "popup_during_fullscreen",
            "popup_during_fullscreen smart ignore",
            "popup_during_fullscreen all",
        ]:
            with self.subTest(directive=directive):
                result = self.translate(directive + "\n")
                self.assertNotIn("popup-during-fullscreen", result.stdout)
                self.assertIn(
                    "expected popup_during_fullscreen smart|ignore|leave_fullscreen",
                    result.stdout,
                )
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_workspace_auto_back_and_forth_uses_sway_boolean_words(self):
        for value in ["1", "yes", "on", "true", "enable", "enabled", "active"]:
            with self.subTest(value=value):
                result = self.translate(f"workspace_auto_back_and_forth {value}\n")
                self.assertIn("workspace-auto-back-and-forth", result.stdout)
                self.assertNotIn("workspace-auto-back-and-forth false", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate("workspace_auto_back_and_forth no\n")
        self.assertIn("workspace-auto-back-and-forth false", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_focus_on_window_activation_translates_sway_modes(self):
        expected = {
            "urgent": "set-urgent",
            "focus": "focus",
            "none": "ignore",
        }
        for mode, action in expected.items():
            with self.subTest(mode=mode):
                result = self.translate(f"focus_on_window_activation {mode}\n")
                self.assertIn(f'on-xdg-activate "{action}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate("focus_on_window_activation smart\n")
        self.assertNotIn("on-xdg-activate", result.stdout)
        self.assertIn("visibility-dependent smart mode", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_no_focus_translates_portable_criteria(self):
        result = self.translate(
            'no_focus [class="^chat$"]\nno_focus [title="^splash$"]\n'
        )
        self.assertIn('match app-id="^chat$"', result.stdout)
        self.assertIn('match title="^splash$"', result.stdout)
        self.assertEqual(result.stdout.count("open-focused false"), 2)
        self.assertIn("manual attention: none", result.stderr)

    def test_no_focus_refuses_unsupported_criteria(self):
        for criterion, reason in [
            ("instance", "X11-only criterion"),
            ("workspace", "workspace criterion"),
        ]:
            with self.subTest(criterion=criterion):
                result = self.translate(f'no_focus [{criterion}="value"]\n')
                self.assertNotIn("window-rule {", result.stdout)
                self.assertIn(reason, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_maps_sway_layer_state_and_refuses_i3_provenance(self):
        for criterion, expected in [("tiling", "false"), ("floating", "true")]:
            with self.subTest(criterion=criterion):
                result = self.translate(
                    f"for_window [{criterion}] floating enable\n"
                )
                self.assertIn(f"match is-floating={expected}", result.stdout)
                self.assertIn("open-floating true", result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        result = self.translate(
            'for_window [class="app" floating] border none\n'
        )
        self.assertIn('match app-id="app" is-floating=true', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        for criterion in ["tiling_from", "floating_from"]:
            for origin in ["auto", "user"]:
                with self.subTest(criterion=criterion, origin=origin):
                    result = self.translate(
                        f'for_window [{criterion}="{origin}"] floating enable\n'
                    )
                    self.assertNotIn("window-rule {", result.stdout)
                    self.assertIn("i3-only provenance criterion", result.stdout)
                    self.assertIn("no sway equivalent", result.stdout)
                    self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_refuses_x11_only_criteria(self):
        for criterion in ["instance", "id", "window_role", "window_type"]:
            with self.subTest(criterion=criterion):
                result = self.translate(
                    f'for_window [{criterion}="value"] floating enable\n'
                )
                self.assertNotIn("window-rule {", result.stdout)
                self.assertIn("X11-only criterion", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_translates_map_time_commands(self):
        result = self.translate('for_window[app_id="mapped"] mark label\n')
        self.assertIn('sway-for-window-command "mark label"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        for action in [
            "kill",
            "reload",
            "move scratchpad",
            "mark label",
            "mark --add label",
            "mark --replace label",
            "mark --add --toggle label",
            "mark --replace --toggle label",
        ]:
            with self.subTest(action=action):
                result = self.translate(f'for_window [app_id="mapped"] {action}\n')
                self.assertIn(f'sway-for-window-command "{action}"', result.stdout)
                self.assertIn("manual attention: none", result.stderr)

        for action in ["mark", "mark --add", "mark --unknown label"]:
            with self.subTest(invalid=action):
                result = self.translate(f'for_window [app_id="mapped"] {action}\n')
                self.assertNotIn("sway-for-window-command", result.stdout)
                self.assertIn("invalid mark command", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_for_window_refuses_missing_rule_surfaces(self):
        for source, reason in [
            ('for_window [workspace="web"] floating enable\n', "workspace criterion"),
            ('for_window [class="foo"] exec notify-send mapped\n', "command needs manual conversion"),
        ]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertNotIn("window-rule {", result.stdout)
                self.assertIn(reason, result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_focus_follows_mouse_maps_exact_modes_and_refuses_always(self):
        result = self.translate("focus_follows_mouse no\n")
        self.assertNotIn("focus-follows-mouse", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        result = self.translate("focus_follows_mouse yes\n")
        self.assertIn("focus-follows-mouse", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        result = self.translate("focus_follows_mouse always\n")
        self.assertNotIn("focus-follows-mouse", result.stdout)
        self.assertIn("focus_follows_mouse always", result.stdout)
        self.assertIn("manual attention: 1 directive(s)", result.stderr)

        for value in ["No", "YeS", "invalid"]:
            with self.subTest(value=value):
                result = self.translate(f"focus_follows_mouse {value}\n")
                self.assertNotIn("focus-follows-mouse", result.stdout)
                self.assertIn("expected focus_follows_mouse no|yes|always", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_mouse_warping_maps_exact_modes_and_refuses_output(self):
        for value, expected in [
            ("none", None),
            ("NoNe", None),
            ("container", 'warp-mouse-to-focus mode="center-xy"'),
            ("CoNtAiNeR", 'warp-mouse-to-focus mode="center-xy"'),
        ]:
            with self.subTest(value=value):
                result = self.translate(f"mouse_warping {value}\n")
                if expected:
                    self.assertIn(expected, result.stdout)
                else:
                    self.assertNotIn("warp-mouse-to-focus", result.stdout)
                self.assertIn("manual attention: none", result.stderr)
        for value in ["output", "OuTpUt", "invalid"]:
            with self.subTest(value=value):
                result = self.translate(f"mouse_warping {value}\n")
                self.assertNotIn("warp-mouse-to-focus", result.stdout)
                self.assertIn("mouse_warping output", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_duplicate_bind_is_reported_instead_of_silently_overwritten(self):
        result = self.translate("bindsym Mod4+h focus left\nbindsym Mod4+h focus right\n")
        self.assertIn('Super+h { command "focus left"; }', result.stdout)
        self.assertNotIn('command "focus right"', result.stdout)
        self.assertIn("duplicate binding for Super+h; kept the first", result.stdout)

    def test_bindcode_is_preserved_as_a_numeric_trigger(self):
        result = self.translate("bindcode --no-repeat 24 kill\n")
        self.assertIn('code:24 repeat=false { command "kill"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_focus_mode_toggle_translates_now_that_the_command_exists(self):
        # The translator used to refuse this specific command with "outside
        # swayward's current command subset". That refusal outlived its reason:
        # Command::FocusModeToggle is implemented (src/command/mod.rs:170) and
        # docs/SWAY_COMPATIBILITY.md already listed mode_toggle as supported.
        result = self.translate("bindsym Mod4+a focus mode_toggle\n")
        self.assertIn('Super+a { command "focus mode_toggle"; }', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_checked_in_sway_and_swayfx_defaults_keep_manual_items(self):
        # The counts are pinned because docs/SWAY_CONFIG_MIGRATION.md quotes
        # them, and it went stale once when a directive started translating.
        # Lower them when coverage improves; a rise is a regression.
        fixtures = ROOT / "tests" / "fixtures" / "config"
        for source, expected in [
            (fixtures / "sway-1.11-default.conf", 3),
            (fixtures / "swayfx-default.conf", 7),
        ]:
            with self.subTest(source=source.name):
                result = subprocess.run(
                    [SCRIPT, source], text=True, capture_output=True, check=True
                )
                self.assertIn(
                    f"manual attention: {expected} directive(s)", result.stderr
                )
                for item in result.stderr.splitlines()[1:]:
                    self.assertIn(item.strip(), result.stdout)

    def test_include_expands_absolute_relative_nested_tilde_glob_and_variables(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / "home"
            home.mkdir()
            fragments = root / "fragments"
            fragments.mkdir()
            (root / "absolute").write_text("bindsym Mod4+a nop absolute\n")
            (root / "relative").write_text("bindsym Mod4+r nop relative\n")
            (root / "nested").write_text("include relative\n")
            (home / "tilde").write_text("bindsym Mod4+t nop tilde\n")
            (fragments / "one").write_text("bindsym Mod4+g nop glob\n")
            (root / "variable").write_text("bindsym Mod4+v nop variable\n")
            (root / "config").write_text(
                f"include {root / 'absolute'}\n"
                "include nested\n"
                "include ~/tilde\n"
                "include fragments/*\n"
                "set $file variable\n"
                "include $file\n"
            )
            result = subprocess.run(
                [SCRIPT, root / "config"],
                text=True,
                capture_output=True,
                check=True,
                env={**os.environ, "HOME": str(home)},
            )
        for command in ["absolute", "relative", "tilde", "glob", "variable"]:
            with self.subTest(command=command):
                self.assertIn(f'nop {command}', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_include_cycle_and_duplicate_real_paths_are_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "child").write_text("bindsym Mod4+h focus left\ninclude config\n")
            (root / "config").write_text("include child\ninclude ./child\n")
            result = subprocess.run(
                [SCRIPT, root / "config"], text=True, capture_output=True, check=True
            )
        self.assertEqual(result.stdout.count('Super+h { command "focus left"; }'), 1)
        self.assertNotIn("include cycle", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_include_refuses_command_substitution_without_executing_it(self):
        for command in ["`touch {marker}`", "$(touch {marker})"]:
            with self.subTest(command=command), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                marker = root / "executed"
                (root / "config").write_text(
                    f"include {command.format(marker=marker)}\n"
                )
                result = subprocess.run(
                    [SCRIPT, root / "config"], text=True, capture_output=True, check=True
                )
                self.assertFalse(marker.exists())
                self.assertIn("command substitution is refused", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)


if __name__ == "__main__":
    unittest.main()
