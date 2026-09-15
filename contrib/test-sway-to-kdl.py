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

    def test_mouse_binding_regions_translate_and_other_options_remain_fail_loud(self):
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

    def test_outer_gap_forms_remain_fail_loud(self):
        for kind in ["outer", "horizontal", "vertical", "top", "right", "bottom", "left"]:
            for value in ["10", "-10px"]:
                with self.subTest(kind=kind, value=value):
                    result = self.translate(f"gaps {kind} {value}\n")
                    self.assertNotIn("struts {", result.stdout)
                    self.assertIn("outer gaps affect floating geometry", result.stdout)
                    self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_gap_forms_refuse_unrepresentable_or_malformed_values(self):
        for source in [
            "gaps inner nope\n",
            "gaps outer 2em\n",
            "gaps diagonal 10\n",
            "gaps outer all set 10px\n",
            "workspace 2 gaps inner 10\n",
        ]:
            with self.subTest(source=source):
                result = self.translate(source)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

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

    def test_deprecated_default_borders_translate_without_changing_shipped_defaults(self):
        result = self.translate("new_window 1pixel\nnew_float normal 3\n")
        self.assertIn("// sway new_window 1pixel", result.stdout)
        self.assertIn('sway-border "pixel"', result.stdout)
        self.assertIn("sway-border-width 1", result.stdout)
        self.assertIn('sway-floating-border "normal"', result.stdout)
        self.assertIn("sway-floating-border-width 3", result.stdout)
        self.assertIn("manual attention: none", result.stderr)
        self.assertNotIn("resources/default-config.kdl", result.stdout)

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

    def test_for_window_combines_class_and_title_without_losing_regex_escapes(self):
        result = self.translate(
            r'''for_window [class="^foo\\w+$" title="^bar\\d+$"] border none
'''
        )
        self.assertIn(r'match app-id="^foo\\\\w+$" title="^bar\\\\d+$"', result.stdout)
        self.assertIn('sway-border "none"', result.stdout)
        self.assertIn("manual attention: none", result.stderr)

    def test_for_window_translates_pixel_border(self):
        result = self.translate('for_window [class="foo"] border 1pixel\n')
        self.assertIn('sway-border "pixel"', result.stdout)
        self.assertIn("sway-border-width 1", result.stdout)
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

    def test_hide_edge_borders_only_maps_the_exact_default(self):
        result = self.translate("hide_edge_borders none\n")
        self.assertIn("hide_edge_borders none (default)", result.stdout)
        self.assertIn("manual attention: none", result.stderr)

        for value in ["vertical", "horizontal", "both", "smart", "smart_no_gaps"]:
            with self.subTest(value=value):
                result = self.translate(f"hide_edge_borders {value}\n")
                self.assertIn("per-edge border suppression", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

        for value in ["none", "vertical", "horizontal", "both", "smart", "smart_no_gaps"]:
            with self.subTest(i3=value):
                result = self.translate(f"hide_edge_borders --i3 {value}\n")
                self.assertIn("hide_lone_tab", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

        for value in ["NONE", "Smart", "bogus", "--i3", "smart extra"]:
            with self.subTest(invalid=value):
                result = self.translate(f"hide_edge_borders {value}\n")
                self.assertIn("expected hide_edge_borders", result.stdout)
                self.assertIn("manual attention: 1 directive(s)", result.stderr)

    def test_popup_during_fullscreen_maps_all_modes_case_insensitively(self):
        for value, expected in [
            ("smart", "smart"),
            ("IGNORE", "ignore"),
            ("Leave_Fullscreen", "leave_fullscreen"),
        ]:
            with self.subTest(value=value):
                result = self.translate(f"popup_during_fullscreen {value}\n")
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
