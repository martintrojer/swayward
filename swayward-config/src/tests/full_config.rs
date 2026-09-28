use insta::assert_debug_snapshot;

use crate::*;

#[track_caller]
fn do_parse(text: &str) -> Config {
    Config::parse_mem(text)
        .map_err(miette::Report::new)
        .unwrap()
}

#[test]
fn parse() {
    let parsed = do_parse(
        r##"
            input {
                keyboard {
                    repeat-delay 600
                    repeat-rate 25
                    track-layout "window"
                    xkb {
                        layout "us,ru"
                        options "grp:win_space_toggle"
                    }
                }

                touchpad {
                    tap
                    dwt
                    dwtp
                    drag true
                    click-method "clickfinger"
                    accel-speed 0.2
                    accel-profile "flat"
                    scroll-method "two-finger"
                    scroll-button 272
                    scroll-button-lock
                    tap-button-map "left-middle-right"
                    disabled-on-external-mouse
                    scroll-factor 0.9
                }

                mouse {
                    natural-scroll
                    accel-speed 0.4
                    accel-profile "flat"
                    scroll-method "no-scroll"
                    scroll-button 273
                    middle-emulation
                    scroll-factor 0.2
                }

                trackpoint {
                    off
                    natural-scroll
                    accel-speed 0.0
                    accel-profile "flat"
                    scroll-method "on-button-down"
                    scroll-button 274
                }

                trackball {
                    off
                    natural-scroll
                    accel-speed 0.0
                    accel-profile "flat"
                    scroll-method "edge"
                    scroll-button 275
                    scroll-button-lock
                    left-handed
                    middle-emulation
                }

                tablet {
                    map-to-output "eDP-1"
                    map-to-focused-output
                    map-to-focused-window
                    calibration-matrix 1.0 2.0 3.0 \
                                       4.0 5.0 6.0
                }

                touch {
                    map-to-output "eDP-1"
                }

                disable-power-key-handling

                warp-mouse-to-focus
                focus-follows-mouse
                workspace-auto-back-and-forth

                mod-key "Mod5"
                mod-key-nested "Super"
            }

            output "eDP-1" {
                focus-at-startup
                scale 2
                transform "flipped-90"
                position x=10 y=20
                mode "1920x1080@144"
                max-bpc 10
                variable-refresh-rate on-demand=true
                background-color "rgba(25, 25, 102, 1.0)"
                hot-corners {
                    off
                    top-left
                    top-right
                    bottom-left
                    bottom-right
                }
            }

            output "eDP-2" {
                mode custom=true "1920x1080@144"
            }

            output "eDP-3" {
                modeline 173.00  1920 2048 2248 2576  1080 1083 1088 1120 "-hsync" "+vsync"
            }

            layout {
                focus-ring {
                    width 5
                    active-color 0 100 200 255
                    inactive-color 255 200 100 0
                    active-gradient from="rgba(10, 20, 30, 1.0)" to="#0080ffff" relative-to="workspace-view"
                }

                border {
                    width 3
                    inactive-color "rgba(255, 200, 100, 0.0)"
                }

                shadow {
                    offset x=10 y=-20
                }

                tab-indicator {
                    width 10
                    position "top"
                }

                preset-column-widths {
                    proportion 0.25
                    proportion 0.5
                    fixed 960
                    fixed 1280
                }

                preset-window-heights {
                    proportion 0.25
                    proportion 0.5
                    fixed 960
                    fixed 1280
                }

                default-column-width { proportion 0.25; }

                gaps 8

                outer-gaps {
                    left -1
                    right 2
                    top 3
                    bottom 4
                }

                struts {
                    left 1
                    right 2
                    top 3
                }

                insert-hint {
                    color "rgb(255, 200, 127)"
                    gradient from="rgba(10, 20, 30, 1.0)" to="#0080ffff" relative-to="workspace-view"
                }
            }

            spawn-at-startup "alacritty" "-e" "fish"
            spawn-sh-at-startup "qs -c ~/source/qs/MyAwesomeShell"

            prefer-no-csd

            cursor {
                xcursor-theme "breeze_cursors"
                xcursor-size 16
                hide-when-typing
                hide-after-inactive-ms 3000
            }

            screenshot-path "~/Screenshots/screenshot.png"

            clipboard {
                disable-primary
            }

            hotkey-overlay {
                skip-at-startup
            }

            animations {
                slowdown 2.0

                workspace-switch {
                    spring damping-ratio=1.0 stiffness=1000 epsilon=0.0001
                }

                window-open { off; }

                window-close {
                    curve "cubic-bezier" 0.05 0.7 0.1 1
                }

                recent-windows-close {
                    off
                }
            }

            gestures {
                dnd-edge-view-scroll {
                    trigger-width 10
                    max-speed 50
                }
            }

            environment {
                QT_QPA_PLATFORM "wayland"
                DISPLAY null
            }

            window-rule {
                match app-id=".*alacritty"
                exclude title="~"
                exclude is-active=true is-focused=false

                open-on-output "eDP-1"
                open-maximized true
                open-fullscreen false
                open-floating false
                open-focused true
                default-window-height { fixed 500; }
                default-column-display "tabbed"
                default-floating-position x=100 y=-200 relative-to="bottom-left"
                on-xdg-activate "ignore"

                focus-ring {
                    off
                    width 3
                }

                border {
                    on
                    width 8.5
                }

                tab-indicator {
                    active-color "#f00"
                }
            }

            layer-rule {
                match namespace="^notifications$"
                block-out-from "screencast"
            }

            binds {
                Mod+Escape hotkey-overlay-title="Inhibit" { toggle-keyboard-shortcuts-inhibit; }
                Mod+Shift+Escape allow-inhibiting=true { toggle-keyboard-shortcuts-inhibit; }
                Mod+T allow-when-locked=true { spawn "alacritty"; }
                Mod+Q hotkey-overlay-title=null { close-window; }
                Mod+Shift+H { focus-monitor-left; }
                Mod+Shift+O { focus-monitor "eDP-1"; }
                Mod+Ctrl+Shift+L { move-window-to-monitor-right; }
                Mod+Ctrl+Alt+O { move-window-to-monitor "eDP-1"; }
                Mod+Ctrl+Alt+P { move-column-to-monitor "DP-1"; }
                Mod+Comma { consume-window-into-column; }
                Mod+1 { focus-workspace 1; }
                Mod+Shift+1 { focus-workspace "workspace-1"; }
                Mod+Shift+E allow-inhibiting=false { quit skip-confirmation=true; }
                Mod+WheelScrollDown cooldown-ms=150 { focus-workspace-down; }
                Super+Alt+S allow-when-locked=true { spawn-sh "pkill orca || exec orca"; }
            }

            switch-events {
                tablet-mode-on { spawn "bash" "-c" "gsettings set org.gnome.desktop.a11y.applications screen-keyboard-enabled true"; }
                tablet-mode-off { spawn "bash" "-c" "gsettings set org.gnome.desktop.a11y.applications screen-keyboard-enabled false"; }
            }

            debug {
                render-drm-device "/dev/dri/renderD129"
                ignore-drm-device "/dev/dri/renderD128"
                ignore-drm-device "/dev/dri/renderD130"
            }

            workspace "workspace-1" {
                open-on-output "eDP-1"
            }
            workspace "workspace-2"
            workspace "workspace-3"

            recent-windows {
                off

                highlight {
                    padding 15
                    active-color "#00ff00"
                }

                previews {
                    max-height 960
                }

                binds {
                    Alt+Tab { next-window; }
                    Alt+grave { next-window filter="app-id"; }
                    Super+Tab { next-window scope="output"; }
                }
            }
            "##,
    );

    assert_debug_snapshot!(parsed, @r#"
        Config {
            input: Input {
                keyboard: Keyboard {
                    xkb: Xkb {
                        rules: "",
                        model: "",
                        layout: "us,ru",
                        variant: "",
                        options: Some(
                            "grp:win_space_toggle",
                        ),
                        file: None,
                    },
                    repeat_delay: 600,
                    repeat_rate: 25,
                    track_layout: Window,
                    numlock: false,
                },
                touchpad: Touchpad {
                    off: false,
                    tap: true,
                    dwt: true,
                    dwtp: true,
                    drag: Some(
                        true,
                    ),
                    drag_lock: false,
                    natural_scroll: false,
                    click_method: Some(
                        Clickfinger,
                    ),
                    accel_speed: FloatOrInt(
                        0.2,
                    ),
                    accel_profile: Some(
                        Flat,
                    ),
                    scroll_method: Some(
                        TwoFinger,
                    ),
                    scroll_button: Some(
                        272,
                    ),
                    scroll_button_lock: true,
                    tap_button_map: Some(
                        LeftMiddleRight,
                    ),
                    left_handed: false,
                    disabled_on_external_mouse: true,
                    middle_emulation: false,
                    scroll_factor: Some(
                        ScrollFactor {
                            base: Some(
                                FloatOrInt(
                                    0.9,
                                ),
                            ),
                            horizontal: None,
                            vertical: None,
                        },
                    ),
                },
                mouse: Mouse {
                    off: false,
                    natural_scroll: true,
                    accel_speed: FloatOrInt(
                        0.4,
                    ),
                    accel_profile: Some(
                        Flat,
                    ),
                    scroll_method: Some(
                        NoScroll,
                    ),
                    scroll_button: Some(
                        273,
                    ),
                    scroll_button_lock: false,
                    left_handed: false,
                    middle_emulation: true,
                    scroll_factor: Some(
                        ScrollFactor {
                            base: Some(
                                FloatOrInt(
                                    0.2,
                                ),
                            ),
                            horizontal: None,
                            vertical: None,
                        },
                    ),
                },
                trackpoint: Trackpoint {
                    off: true,
                    natural_scroll: true,
                    accel_speed: FloatOrInt(
                        0.0,
                    ),
                    accel_profile: Some(
                        Flat,
                    ),
                    scroll_method: Some(
                        OnButtonDown,
                    ),
                    scroll_button: Some(
                        274,
                    ),
                    scroll_button_lock: false,
                    left_handed: false,
                    middle_emulation: false,
                },
                trackball: Trackball {
                    off: true,
                    natural_scroll: true,
                    accel_speed: FloatOrInt(
                        0.0,
                    ),
                    accel_profile: Some(
                        Flat,
                    ),
                    scroll_method: Some(
                        Edge,
                    ),
                    scroll_button: Some(
                        275,
                    ),
                    scroll_button_lock: true,
                    left_handed: true,
                    middle_emulation: true,
                },
                tablet: Tablet {
                    off: false,
                    calibration_matrix: Some(
                        [
                            1.0,
                            2.0,
                            3.0,
                            4.0,
                            5.0,
                            6.0,
                        ],
                    ),
                    map_to_output: Some(
                        "eDP-1",
                    ),
                    map_to_focused_output: true,
                    map_to_focused_window: true,
                    left_handed: false,
                },
                touch: Touch {
                    off: false,
                    calibration_matrix: None,
                    map_to_output: Some(
                        "eDP-1",
                    ),
                },
                disable_power_key_handling: true,
                warp_mouse_to_focus: Some(
                    WarpMouseToFocus {
                        mode: None,
                    },
                ),
                focus_follows_mouse: Some(
                    FocusFollowsMouse {
                        mode: Yes,
                        max_scroll_amount: None,
                    },
                ),
                mouse_warping: No,
                floating_modifier: None,
                workspace_auto_back_and_forth: true,
                tiling_drag: true,
                tiling_drag_threshold: 9,
                border_resize: true,
                gap_resize: false,
                mod_key: Some(
                    IsoLevel3Shift,
                ),
                mod_key_nested: Some(
                    Super,
                ),
            },
            outputs: Outputs(
                [
                    Output {
                        off: false,
                        name: "eDP-1",
                        scale: Some(
                            FloatOrInt(
                                2.0,
                            ),
                        ),
                        transform: Flipped90,
                        position: Some(
                            Position {
                                x: 10,
                                y: 20,
                            },
                        ),
                        max_bpc: Some(
                            MaxBpc(
                                _10,
                            ),
                        ),
                        mode: Some(
                            Mode {
                                custom: false,
                                mode: ConfiguredMode {
                                    width: 1920,
                                    height: 1080,
                                    refresh: Some(
                                        144.0,
                                    ),
                                },
                            },
                        ),
                        modeline: None,
                        variable_refresh_rate: Some(
                            Vrr {
                                on_demand: true,
                            },
                        ),
                        focus_at_startup: true,
                        background_color: Some(
                            Color {
                                r: 0.09803922,
                                g: 0.09803922,
                                b: 0.4,
                                a: 1.0,
                            },
                        ),
                        backdrop_color: None,
                        hot_corners: Some(
                            HotCorners {
                                off: true,
                                top_left: true,
                                top_right: true,
                                bottom_left: true,
                                bottom_right: true,
                            },
                        ),
                        layout: None,
                    },
                    Output {
                        off: false,
                        name: "eDP-2",
                        scale: None,
                        transform: Normal,
                        position: None,
                        max_bpc: None,
                        mode: Some(
                            Mode {
                                custom: true,
                                mode: ConfiguredMode {
                                    width: 1920,
                                    height: 1080,
                                    refresh: Some(
                                        144.0,
                                    ),
                                },
                            },
                        ),
                        modeline: None,
                        variable_refresh_rate: None,
                        focus_at_startup: false,
                        background_color: None,
                        backdrop_color: None,
                        hot_corners: None,
                        layout: None,
                    },
                    Output {
                        off: false,
                        name: "eDP-3",
                        scale: None,
                        transform: Normal,
                        position: None,
                        max_bpc: None,
                        mode: None,
                        modeline: Some(
                            Modeline {
                                clock: 173.0,
                                hdisplay: 1920,
                                hsync_start: 2048,
                                hsync_end: 2248,
                                htotal: 2576,
                                vdisplay: 1080,
                                vsync_start: 1083,
                                vsync_end: 1088,
                                vtotal: 1120,
                                hsync_polarity: NHSync,
                                vsync_polarity: PVSync,
                            },
                        ),
                        variable_refresh_rate: None,
                        focus_at_startup: false,
                        background_color: None,
                        backdrop_color: None,
                        hot_corners: None,
                        layout: None,
                    },
                ],
            ),
            spawn_at_startup: [
                SpawnAtStartup {
                    command: [
                        "alacritty",
                        "-e",
                        "fish",
                    ],
                },
            ],
            spawn_sh_at_startup: [
                SpawnShAtStartup {
                    command: "qs -c ~/source/qs/MyAwesomeShell",
                },
            ],
            layout: Layout {
                focus_ring: FocusRing {
                    off: false,
                    width: 5.0,
                    active_color: Color {
                        r: 0.0,
                        g: 0.39215687,
                        b: 0.78431374,
                        a: 1.0,
                    },
                    inactive_color: Color {
                        r: 1.0,
                        g: 0.78431374,
                        b: 0.39215687,
                        a: 0.0,
                    },
                    urgent_color: Color {
                        r: 0.60784316,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    },
                    active_gradient: Some(
                        Gradient {
                            from: Color {
                                r: 0.039215688,
                                g: 0.078431375,
                                b: 0.11764706,
                                a: 1.0,
                            },
                            to: Color {
                                r: 0.0,
                                g: 0.5019608,
                                b: 1.0,
                                a: 1.0,
                            },
                            angle: 180,
                            relative_to: WorkspaceView,
                            in_: GradientInterpolation {
                                color_space: Srgb,
                                hue_interpolation: Shorter,
                            },
                        },
                    ),
                    inactive_gradient: None,
                    urgent_gradient: None,
                },
                border: Border {
                    off: false,
                    width: 3.0,
                    active_color: Color {
                        r: 1.0,
                        g: 0.78431374,
                        b: 0.49803922,
                        a: 1.0,
                    },
                    inactive_color: Color {
                        r: 1.0,
                        g: 0.78431374,
                        b: 0.39215687,
                        a: 0.0,
                    },
                    urgent_color: Color {
                        r: 0.60784316,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    },
                    active_gradient: None,
                    inactive_gradient: None,
                    urgent_gradient: None,
                },
                shadow: Shadow {
                    on: false,
                    offset: ShadowOffset {
                        x: FloatOrInt(
                            10.0,
                        ),
                        y: FloatOrInt(
                            -20.0,
                        ),
                    },
                    softness: 30.0,
                    spread: 5.0,
                    draw_behind_window: false,
                    color: Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.46666667,
                    },
                    inactive_color: None,
                },
                tab_indicator: TabIndicator {
                    off: false,
                    hide_when_single_tab: false,
                    place_within_column: false,
                    gap: 5.0,
                    width: 10.0,
                    length: TabIndicatorLength {
                        total_proportion: Some(
                            0.5,
                        ),
                    },
                    position: Top,
                    gaps_between_tabs: 0.0,
                    corner_radius: 0.0,
                    active_color: None,
                    inactive_color: None,
                    urgent_color: None,
                    active_gradient: None,
                    inactive_gradient: None,
                    urgent_gradient: None,
                },
                titlebar: Titlebar {
                    font: "monospace 10",
                    pango_markup: false,
                    show_marks: true,
                    alignment: Left,
                    horizontal_padding: 5.0,
                    vertical_padding: 4.0,
                    border_thickness: 1,
                    focused: TitlebarColors {
                        border_color: Color {
                            r: 0.29803923,
                            g: 0.47058824,
                            b: 0.6,
                            a: 1.0,
                        },
                        background_color: Color {
                            r: 0.15686275,
                            g: 0.33333334,
                            b: 0.46666667,
                            a: 1.0,
                        },
                        text_color: Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        },
                    },
                    focused_inactive: TitlebarColors {
                        border_color: Color {
                            r: 0.2,
                            g: 0.2,
                            b: 0.2,
                            a: 1.0,
                        },
                        background_color: Color {
                            r: 0.37254903,
                            g: 0.40392157,
                            b: 0.41568628,
                            a: 1.0,
                        },
                        text_color: Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        },
                    },
                    focused_tab_title: TitlebarColors {
                        border_color: Color {
                            r: 0.2,
                            g: 0.2,
                            b: 0.2,
                            a: 1.0,
                        },
                        background_color: Color {
                            r: 0.37254903,
                            g: 0.40392157,
                            b: 0.41568628,
                            a: 1.0,
                        },
                        text_color: Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        },
                    },
                    unfocused: TitlebarColors {
                        border_color: Color {
                            r: 0.2,
                            g: 0.2,
                            b: 0.2,
                            a: 1.0,
                        },
                        background_color: Color {
                            r: 0.13333334,
                            g: 0.13333334,
                            b: 0.13333334,
                            a: 1.0,
                        },
                        text_color: Color {
                            r: 0.53333336,
                            g: 0.53333336,
                            b: 0.53333336,
                            a: 1.0,
                        },
                    },
                    urgent: TitlebarColors {
                        border_color: Color {
                            r: 0.18431373,
                            g: 0.20392157,
                            b: 0.22745098,
                            a: 1.0,
                        },
                        background_color: Color {
                            r: 0.5647059,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        },
                        text_color: Color {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 1.0,
                        },
                    },
                },
                draw_uncovered_top_border: true,
                insert_hint: InsertHint {
                    off: false,
                    color: Color {
                        r: 1.0,
                        g: 0.78431374,
                        b: 0.49803922,
                        a: 1.0,
                    },
                    gradient: Some(
                        Gradient {
                            from: Color {
                                r: 0.039215688,
                                g: 0.078431375,
                                b: 0.11764706,
                                a: 1.0,
                            },
                            to: Color {
                                r: 0.0,
                                g: 0.5019608,
                                b: 1.0,
                                a: 1.0,
                            },
                            angle: 180,
                            relative_to: WorkspaceView,
                            in_: GradientInterpolation {
                                color_space: Srgb,
                                hue_interpolation: Shorter,
                            },
                        },
                    ),
                },
                preset_column_widths: [
                    Proportion(
                        0.25,
                    ),
                    Proportion(
                        0.5,
                    ),
                    Fixed(
                        960,
                    ),
                    Fixed(
                        1280,
                    ),
                ],
                default_column_width: Some(
                    Proportion(
                        0.25,
                    ),
                ),
                preset_window_heights: [
                    Proportion(
                        0.25,
                    ),
                    Proportion(
                        0.5,
                    ),
                    Fixed(
                        960,
                    ),
                    Fixed(
                        1280,
                    ),
                ],
                focus_wrapping: Yes,
                workspace_layout: Default,
                default_orientation: Auto,
                hide_edge_borders: None,
                smart_borders: Off,
                smart_gaps: Off,
                default_border: SwayBorderDefault {
                    style: Normal,
                    width: None,
                },
                default_floating_border: SwayBorderDefault {
                    style: Normal,
                    width: None,
                },
                floating_minimum_size: FloatingSize {
                    width: 75,
                    height: 50,
                },
                floating_maximum_size: FloatingSize {
                    width: 0,
                    height: 0,
                },
                gaps: 8.0,
                outer_gaps: OuterGaps {
                    left: -1.0,
                    right: 2.0,
                    top: 3.0,
                    bottom: 4.0,
                },
                outer_gaps_configured: true,
                struts: Struts {
                    left: FloatOrInt(
                        1.0,
                    ),
                    right: FloatOrInt(
                        2.0,
                    ),
                    top: FloatOrInt(
                        3.0,
                    ),
                    bottom: FloatOrInt(
                        0.0,
                    ),
                },
                background_color: Color {
                    r: 0.25,
                    g: 0.25,
                    b: 0.25,
                    a: 1.0,
                },
            },
            prefer_no_csd: true,
            popup_during_fullscreen: Smart,
            focus_on_window_activation: Urgent,
            urgent_timeout_ms: 500,
            cursor: Cursor {
                xcursor_theme: "breeze_cursors",
                xcursor_size: 16,
                hide_when_typing: true,
                hide_after_inactive_ms: Some(
                    3000,
                ),
            },
            screenshot_path: ScreenshotPath(
                Some(
                    "~/Screenshots/screenshot.png",
                ),
            ),
            clipboard: Clipboard {
                disable_primary: true,
            },
            hotkey_overlay: HotkeyOverlay {
                skip_at_startup: true,
                hide_not_bound: false,
            },
            config_notification: ConfigNotification {
                disable_failed: false,
            },
            animations: Animations {
                off: false,
                slowdown: 2.0,
                workspace_switch: WorkspaceSwitchAnim(
                    Animation {
                        off: false,
                        kind: Spring(
                            SpringParams {
                                damping_ratio: 1.0,
                                stiffness: 1000,
                                epsilon: 0.0001,
                            },
                        ),
                    },
                ),
                window_open: WindowOpenAnim {
                    anim: Animation {
                        off: true,
                        kind: Easing(
                            EasingParams {
                                duration_ms: 150,
                                curve: EaseOutExpo,
                            },
                        ),
                    },
                    custom_shader: None,
                },
                window_close: WindowCloseAnim {
                    anim: Animation {
                        off: false,
                        kind: Easing(
                            EasingParams {
                                duration_ms: 150,
                                curve: CubicBezier(
                                    0.05,
                                    0.7,
                                    0.1,
                                    1.0,
                                ),
                            },
                        ),
                    },
                    custom_shader: None,
                },
                window_movement: WindowMovementAnim(
                    Animation {
                        off: false,
                        kind: Spring(
                            SpringParams {
                                damping_ratio: 1.0,
                                stiffness: 800,
                                epsilon: 0.0001,
                            },
                        ),
                    },
                ),
                window_resize: WindowResizeAnim {
                    anim: Animation {
                        off: false,
                        kind: Spring(
                            SpringParams {
                                damping_ratio: 1.0,
                                stiffness: 800,
                                epsilon: 0.0001,
                            },
                        ),
                    },
                    custom_shader: None,
                },
                config_notification_open_close: ConfigNotificationOpenCloseAnim(
                    Animation {
                        off: false,
                        kind: Spring(
                            SpringParams {
                                damping_ratio: 0.6,
                                stiffness: 1000,
                                epsilon: 0.001,
                            },
                        ),
                    },
                ),
                exit_confirmation_open_close: ExitConfirmationOpenCloseAnim(
                    Animation {
                        off: false,
                        kind: Spring(
                            SpringParams {
                                damping_ratio: 0.6,
                                stiffness: 500,
                                epsilon: 0.01,
                            },
                        ),
                    },
                ),
                screenshot_ui_open: ScreenshotUiOpenAnim(
                    Animation {
                        off: false,
                        kind: Easing(
                            EasingParams {
                                duration_ms: 200,
                                curve: EaseOutQuad,
                            },
                        ),
                    },
                ),
                overview_open_close: OverviewOpenCloseAnim(
                    Animation {
                        off: false,
                        kind: Spring(
                            SpringParams {
                                damping_ratio: 1.0,
                                stiffness: 800,
                                epsilon: 0.0001,
                            },
                        ),
                    },
                ),
                recent_windows_close: RecentWindowsCloseAnim(
                    Animation {
                        off: true,
                        kind: Spring(
                            SpringParams {
                                damping_ratio: 1.0,
                                stiffness: 800,
                                epsilon: 0.001,
                            },
                        ),
                    },
                ),
            },
            blur: Blur {
                off: false,
                passes: 3,
                offset: 3.0,
                noise: 0.02,
                saturation: 1.5,
            },
            gestures: Gestures {
                dnd_edge_view_scroll: DndEdgeViewScroll {
                    trigger_width: 10.0,
                    delay_ms: 100,
                    max_speed: 50.0,
                },
                dnd_edge_workspace_switch: DndEdgeWorkspaceSwitch {
                    trigger_height: 50.0,
                    delay_ms: 100,
                    max_speed: 1500.0,
                },
                hot_corners: HotCorners {
                    off: false,
                    top_left: false,
                    top_right: false,
                    bottom_left: false,
                    bottom_right: false,
                },
            },
            overview: Overview {
                zoom: 0.5,
                backdrop_color: Color {
                    r: 0.15,
                    g: 0.15,
                    b: 0.15,
                    a: 1.0,
                },
                workspace_shadow: WorkspaceShadow {
                    off: false,
                    offset: ShadowOffset {
                        x: FloatOrInt(
                            0.0,
                        ),
                        y: FloatOrInt(
                            10.0,
                        ),
                    },
                    softness: 40.0,
                    spread: 10.0,
                    color: Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.3137255,
                    },
                },
            },
            environment: Environment(
                [
                    EnvironmentVariable {
                        name: "QT_QPA_PLATFORM",
                        value: Some(
                            "wayland",
                        ),
                    },
                    EnvironmentVariable {
                        name: "DISPLAY",
                        value: None,
                    },
                ],
            ),
            xwayland_satellite: XwaylandSatellite {
                off: false,
                path: "xwayland-satellite",
            },
            window_rules: [
                WindowRule {
                    matches: [
                        Match {
                            app_id: Some(
                                RegexEq(
                                    Regex(
                                        ".*alacritty",
                                    ),
                                ),
                            ),
                            title: None,
                            is_active: None,
                            is_focused: None,
                            is_active_in_column: None,
                            is_floating: None,
                            is_window_cast_target: None,
                            is_urgent: None,
                            at_startup: None,
                        },
                    ],
                    excludes: [
                        Match {
                            app_id: None,
                            title: Some(
                                RegexEq(
                                    Regex(
                                        "~",
                                    ),
                                ),
                            ),
                            is_active: None,
                            is_focused: None,
                            is_active_in_column: None,
                            is_floating: None,
                            is_window_cast_target: None,
                            is_urgent: None,
                            at_startup: None,
                        },
                        Match {
                            app_id: None,
                            title: None,
                            is_active: Some(
                                true,
                            ),
                            is_focused: Some(
                                false,
                            ),
                            is_active_in_column: None,
                            is_floating: None,
                            is_window_cast_target: None,
                            is_urgent: None,
                            at_startup: None,
                        },
                    ],
                    default_column_width: None,
                    default_window_height: Some(
                        DefaultPresetSize(
                            Some(
                                Fixed(
                                    500,
                                ),
                            ),
                        ),
                    ),
                    open_on_output: Some(
                        "eDP-1",
                    ),
                    open_on_workspace: None,
                    open_on_workspace_number: None,
                    open_maximized: Some(
                        true,
                    ),
                    open_maximized_to_edges: None,
                    open_fullscreen: Some(
                        false,
                    ),
                    open_floating: Some(
                        false,
                    ),
                    open_focused: Some(
                        true,
                    ),
                    sway_border: None,
                    sway_border_width: None,
                    sway_floating_border: None,
                    sway_floating_border_width: None,
                    on_xdg_activate: Some(
                        Ignore,
                    ),
                    sway_for_window_commands: [],
                    min_width: None,
                    min_height: None,
                    max_width: None,
                    max_height: None,
                    focus_ring: BorderRule {
                        off: true,
                        on: false,
                        width: Some(
                            FloatOrInt(
                                3.0,
                            ),
                        ),
                        active_color: None,
                        inactive_color: None,
                        urgent_color: None,
                        active_gradient: None,
                        inactive_gradient: None,
                        urgent_gradient: None,
                    },
                    border: BorderRule {
                        off: false,
                        on: true,
                        width: Some(
                            FloatOrInt(
                                8.5,
                            ),
                        ),
                        active_color: None,
                        inactive_color: None,
                        urgent_color: None,
                        active_gradient: None,
                        inactive_gradient: None,
                        urgent_gradient: None,
                    },
                    shadow: ShadowRule {
                        off: false,
                        on: false,
                        offset: None,
                        softness: None,
                        spread: None,
                        draw_behind_window: None,
                        color: None,
                        inactive_color: None,
                    },
                    tab_indicator: TabIndicatorRule {
                        active_color: Some(
                            Color {
                                r: 1.0,
                                g: 0.0,
                                b: 0.0,
                                a: 1.0,
                            },
                        ),
                        inactive_color: None,
                        urgent_color: None,
                        active_gradient: None,
                        inactive_gradient: None,
                        urgent_gradient: None,
                    },
                    draw_border_with_background: None,
                    opacity: None,
                    geometry_corner_radius: None,
                    clip_to_geometry: None,
                    baba_is_float: None,
                    block_out_from: None,
                    variable_refresh_rate: None,
                    default_column_display: Some(
                        Tabbed,
                    ),
                    default_floating_position: Some(
                        FloatingPosition {
                            x: FloatOrInt(
                                100.0,
                            ),
                            y: FloatOrInt(
                                -200.0,
                            ),
                            relative_to: BottomLeft,
                        },
                    ),
                    scroll_factor: None,
                    tiled_state: None,
                    background_effect: BackgroundEffectRule {
                        xray: None,
                        blur: None,
                        noise: None,
                        saturation: None,
                    },
                    popups: PopupsRule {
                        opacity: None,
                        geometry_corner_radius: None,
                        background_effect: BackgroundEffectRule {
                            xray: None,
                            blur: None,
                            noise: None,
                            saturation: None,
                        },
                    },
                },
            ],
            layer_rules: [
                LayerRule {
                    matches: [
                        Match {
                            namespace: Some(
                                RegexEq(
                                    Regex(
                                        "^notifications$",
                                    ),
                                ),
                            ),
                            at_startup: None,
                            layer: None,
                        },
                    ],
                    excludes: [],
                    opacity: None,
                    block_out_from: Some(
                        Screencast,
                    ),
                    shadow: ShadowRule {
                        off: false,
                        on: false,
                        offset: None,
                        softness: None,
                        spread: None,
                        draw_behind_window: None,
                        color: None,
                        inactive_color: None,
                    },
                    geometry_corner_radius: None,
                    place_within_backdrop: None,
                    baba_is_float: None,
                    background_effect: BackgroundEffectRule {
                        xray: None,
                        blur: None,
                        noise: None,
                        saturation: None,
                    },
                    popups: PopupsRule {
                        opacity: None,
                        geometry_corner_radius: None,
                        background_effect: BackgroundEffectRule {
                            xray: None,
                            blur: None,
                            noise: None,
                            saturation: None,
                        },
                    },
                },
            ],
            binds: Binds(
                [
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_Escape,
                            ),
                            modifiers: Modifiers(
                                COMPOSITOR,
                            ),
                        },
                        action: ToggleKeyboardShortcutsInhibit,
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: false,
                        hotkey_overlay_title: Some(
                            Some(
                                "Inhibit",
                            ),
                        ),
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_Escape,
                            ),
                            modifiers: Modifiers(
                                SHIFT | COMPOSITOR,
                            ),
                        },
                        action: ToggleKeyboardShortcutsInhibit,
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: false,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_t,
                            ),
                            modifiers: Modifiers(
                                COMPOSITOR,
                            ),
                        },
                        action: Spawn(
                            [
                                "alacritty",
                            ],
                        ),
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: true,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_q,
                            ),
                            modifiers: Modifiers(
                                COMPOSITOR,
                            ),
                        },
                        action: CloseWindow,
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: Some(
                            None,
                        ),
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_h,
                            ),
                            modifiers: Modifiers(
                                SHIFT | COMPOSITOR,
                            ),
                        },
                        action: FocusMonitorLeft,
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_o,
                            ),
                            modifiers: Modifiers(
                                SHIFT | COMPOSITOR,
                            ),
                        },
                        action: FocusMonitor(
                            "eDP-1",
                        ),
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_l,
                            ),
                            modifiers: Modifiers(
                                CTRL | SHIFT | COMPOSITOR,
                            ),
                        },
                        action: MoveWindowToMonitorRight,
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_o,
                            ),
                            modifiers: Modifiers(
                                CTRL | ALT | COMPOSITOR,
                            ),
                        },
                        action: MoveWindowToMonitor(
                            "eDP-1",
                        ),
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_p,
                            ),
                            modifiers: Modifiers(
                                CTRL | ALT | COMPOSITOR,
                            ),
                        },
                        action: MoveColumnToMonitor(
                            "DP-1",
                        ),
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_comma,
                            ),
                            modifiers: Modifiers(
                                COMPOSITOR,
                            ),
                        },
                        action: ConsumeWindowIntoColumn,
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_1,
                            ),
                            modifiers: Modifiers(
                                COMPOSITOR,
                            ),
                        },
                        action: FocusWorkspace(
                            Index(
                                1,
                            ),
                        ),
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_1,
                            ),
                            modifiers: Modifiers(
                                SHIFT | COMPOSITOR,
                            ),
                        },
                        action: FocusWorkspace(
                            Name(
                                "workspace-1",
                            ),
                        ),
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_e,
                            ),
                            modifiers: Modifiers(
                                SHIFT | COMPOSITOR,
                            ),
                        },
                        action: Quit(
                            true,
                        ),
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: false,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: WheelScrollDown,
                            modifiers: Modifiers(
                                COMPOSITOR,
                            ),
                        },
                        action: FocusWorkspaceDown,
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: Some(
                            150ms,
                        ),
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_s,
                            ),
                            modifiers: Modifiers(
                                ALT | SUPER,
                            ),
                        },
                        action: SpawnSh(
                            "pkill orca || exec orca",
                        ),
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: true,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                ],
            ),
            binding_modes: [],
            switch_events: SwitchBinds {
                lid_open: None,
                lid_close: None,
                tablet_mode_on: Some(
                    SwitchAction {
                        spawn: [
                            "bash",
                            "-c",
                            "gsettings set org.gnome.desktop.a11y.applications screen-keyboard-enabled true",
                        ],
                    },
                ),
                tablet_mode_off: Some(
                    SwitchAction {
                        spawn: [
                            "bash",
                            "-c",
                            "gsettings set org.gnome.desktop.a11y.applications screen-keyboard-enabled false",
                        ],
                    },
                ),
            },
            debug: Debug {
                preview_render: None,
                dbus_interfaces_in_non_session_instances: false,
                wait_for_frame_completion_before_queueing: false,
                enable_overlay_planes: false,
                disable_cursor_plane: false,
                disable_direct_scanout: false,
                restrict_primary_scanout_to_matching_format: false,
                force_disable_connectors_on_resume: false,
                render_drm_device: Some(
                    "/dev/dri/renderD129",
                ),
                ignored_drm_devices: [
                    "/dev/dri/renderD128",
                    "/dev/dri/renderD130",
                ],
                force_pipewire_invalid_modifier: false,
                emulate_zero_presentation_time: false,
                disable_resize_throttling: false,
                disable_transactions: false,
                keep_laptop_panel_on_when_lid_is_closed: false,
                disable_monitor_names: false,
                strict_new_window_focus_policy: false,
                honor_xdg_activation_with_invalid_serial: false,
                deactivate_unfocused_windows: true,
                skip_cursor_only_updates_during_vrr: false,
                disable_10bit_output: false,
            },
            workspaces: [
                Workspace {
                    name: WorkspaceName(
                        "workspace-1",
                    ),
                    sway_output_assignment: None,
                    open_on_output: Some(
                        "eDP-1",
                    ),
                    layout: None,
                },
                Workspace {
                    name: WorkspaceName(
                        "workspace-2",
                    ),
                    sway_output_assignment: None,
                    open_on_output: None,
                    layout: None,
                },
                Workspace {
                    name: WorkspaceName(
                        "workspace-3",
                    ),
                    sway_output_assignment: None,
                    open_on_output: None,
                    layout: None,
                },
            ],
            recent_windows: RecentWindows {
                on: false,
                debounce_ms: 750,
                open_delay_ms: 150,
                highlight: MruHighlight {
                    active_color: Color {
                        r: 0.0,
                        g: 1.0,
                        b: 0.0,
                        a: 1.0,
                    },
                    urgent_color: Color {
                        r: 1.0,
                        g: 0.6,
                        b: 0.6,
                        a: 1.0,
                    },
                    padding: 15.0,
                    corner_radius: 0.0,
                },
                previews: MruPreviews {
                    max_height: 960.0,
                    max_scale: 0.5,
                },
                binds: [
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_Tab,
                            ),
                            modifiers: Modifiers(
                                ALT,
                            ),
                        },
                        action: MruAdvance {
                            direction: Forward,
                            scope: None,
                            filter: Some(
                                All,
                            ),
                        },
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_grave,
                            ),
                            modifiers: Modifiers(
                                ALT,
                            ),
                        },
                        action: MruAdvance {
                            direction: Forward,
                            scope: None,
                            filter: Some(
                                AppId,
                            ),
                        },
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                    Bind {
                        key: Key {
                            trigger: Keysym(
                                XK_Tab,
                            ),
                            modifiers: Modifiers(
                                SUPER,
                            ),
                        },
                        action: MruAdvance {
                            direction: Forward,
                            scope: Some(
                                Output,
                            ),
                            filter: Some(
                                All,
                            ),
                        },
                        mouse_regions: MouseRegions(
                            0x0,
                        ),
                        input_device: "*",
                        group: None,
                        release: false,
                        repeat: true,
                        cooldown: None,
                        allow_when_locked: false,
                        allow_inhibiting: true,
                        hotkey_overlay_title: None,
                    },
                ],
            },
        }
        "#);
}
