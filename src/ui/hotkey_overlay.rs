use std::cell::RefCell;
use std::cmp::max;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::iter::zip;
use std::rc::Rc;

use pangocairo::cairo::{self, ImageSurface};
use pangocairo::pango::{AttrColor, AttrInt, AttrList, AttrString, FontDescription, Weight};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::input::keyboard::xkb::keysym_get_name;
use smithay::output::{Output, WeakOutput};
use smithay::reexports::gbm::Format as Fourcc;
use smithay::utils::{Scale, Transform};
use swayward_config::{Action, Bind, Config, Key, ModKey, Modifiers, Trigger};

use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::texture::{TextureBuffer, TextureRenderElement};
use crate::utils::{output_size, to_physical_precise_round};

const PADDING: i32 = 8;
// const MARGIN: i32 = PADDING * 2;
const FONT: &str = "sans 14px";
const BORDER: i32 = 4;
const LINE_INTERVAL: i32 = 2;
const TITLE: &str = "Important Hotkeys";

pub struct HotkeyOverlay {
    is_open: bool,
    config: Rc<RefCell<Config>>,
    mod_key: ModKey,
    buffers: RefCell<HashMap<WeakOutput, RenderedOverlay>>,
}

pub struct RenderedOverlay {
    buffer: Option<TextureBuffer<GlesTexture>>,
}

impl HotkeyOverlay {
    pub fn new(config: Rc<RefCell<Config>>, mod_key: ModKey) -> Self {
        Self {
            is_open: false,
            config,
            mod_key,
            buffers: RefCell::new(HashMap::new()),
        }
    }

    pub fn show(&mut self) -> bool {
        if !self.is_open {
            self.is_open = true;
            true
        } else {
            false
        }
    }

    pub fn hide(&mut self) -> bool {
        if self.is_open {
            self.is_open = false;
            true
        } else {
            false
        }
    }

    pub fn is_open(&self) -> bool {
        self.is_open
    }

    pub fn on_hotkey_config_updated(&mut self, mod_key: ModKey) {
        self.mod_key = mod_key;
        self.buffers.borrow_mut().clear();
    }

    pub fn render<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Option<PrimaryGpuTextureRenderElement> {
        if !self.is_open {
            return None;
        }

        let scale = output.current_scale().fractional_scale();
        let output_size = output_size(output);

        let mut buffers = self.buffers.borrow_mut();
        buffers.retain(|output, _| output.is_alive());

        // FIXME: should probably use the working area rather than view size.
        let weak = output.downgrade();
        if let Some(rendered) = buffers.get(&weak) {
            if let Some(buffer) = &rendered.buffer {
                if buffer.texture_scale() != Scale::from(scale) {
                    buffers.remove(&weak);
                }
            }
        }

        let rendered = buffers.entry(weak).or_insert_with(|| {
            let renderer = renderer.as_gles_renderer();
            render(renderer, &self.config.borrow(), self.mod_key, scale)
                .unwrap_or_else(|_| RenderedOverlay { buffer: None })
        });
        let buffer = rendered.buffer.as_ref()?;

        let size = buffer.logical_size();
        let location = (output_size.to_f64().to_point() - size.to_point()).downscale(2.);
        let mut location = location.to_physical_precise_round(scale).to_logical(scale);
        location.x = f64::max(0., location.x);
        location.y = f64::max(0., location.y);

        let elem = TextureRenderElement::from_texture_buffer(
            buffer.clone(),
            location,
            0.9,
            None,
            None,
            Kind::Unspecified,
        );

        Some(PrimaryGpuTextureRenderElement(elem))
    }

    pub fn a11y_text(&self) -> String {
        let config = self.config.borrow();
        let entries = collect_entries(&config);

        let mut buf = String::new();
        writeln!(&mut buf, "{TITLE}").unwrap();

        for entry in entries {
            let keys = entry
                .keys
                .iter()
                .map(|key| key_name(true, self.mod_key, key))
                .collect::<Vec<_>>()
                .join(" / ");
            let label = match pango::parse_markup(&entry.label, '\0') {
                Ok((_attrs, text, _accel)) => text,
                Err(_) => entry.label.into(),
            };

            writeln!(&mut buf, "{keys} {label}").unwrap();
        }

        buf
    }
}

fn format_bind(binds: &[Bind], action: &Action) -> Option<(Option<Key>, String)> {
    let mut bind_with_non_null = None;
    let mut bind_with_custom_title = None;
    let mut found_null_title = false;

    for bind in binds {
        if bind.action != *action {
            continue;
        }

        match &bind.hotkey_overlay_title {
            Some(Some(_)) => {
                bind_with_custom_title.get_or_insert(bind);
            }
            Some(None) => {
                found_null_title = true;
            }
            None => {
                bind_with_non_null.get_or_insert(bind);
            }
        }
    }

    if bind_with_custom_title.is_none() && found_null_title {
        return None;
    }

    let mut title = None;
    let key = if let Some(bind) = bind_with_custom_title.or(bind_with_non_null) {
        if let Some(Some(custom)) = &bind.hotkey_overlay_title {
            title = Some(custom.clone());
        }

        Some(bind.key)
    } else {
        None
    };
    let title = title.unwrap_or_else(|| action_name(action));

    Some((key, title))
}

#[derive(Debug, PartialEq)]
struct Entry {
    keys: Vec<Key>,
    label: String,
}

const COMMAND_GROUPS: &[&[&str]] = &[
    &["exec foot"],
    &["exec fuzzel"],
    &["kill"],
    &["focus left", "focus down", "focus up", "focus right"],
    &["move left", "move down", "move up", "move right"],
    &["split h", "split v"],
    &["layout tabbed"],
    &["layout stacking"],
    &["layout toggle split"],
    &["focus parent"],
    &["fullscreen"],
    &["floating toggle"],
    &[
        "workspace number 1",
        "workspace number 2",
        "workspace number 3",
        "workspace number 4",
        "workspace number 5",
        "workspace number 6",
        "workspace number 7",
        "workspace number 8",
        "workspace number 9",
        "workspace number 10",
    ],
    &[
        "move to workspace number 1",
        "move to workspace number 2",
        "move to workspace number 3",
        "move to workspace number 4",
        "move to workspace number 5",
        "move to workspace number 6",
        "move to workspace number 7",
        "move to workspace number 8",
        "move to workspace number 9",
        "move to workspace number 10",
    ],
    &["mode resize"],
    &["move scratchpad"],
    &["scratchpad show"],
];

fn collect_entries(config: &Config) -> Vec<Entry> {
    let binds = &config.binds.0;
    let mut entries = COMMAND_GROUPS
        .iter()
        .filter_map(|commands| {
            let matching = commands
                .iter()
                .filter_map(|command| {
                    binds.iter().find(|bind| {
                        matches!(&bind.action, Action::SwayCommand(bound) if bound == command)
                            && bind.hotkey_overlay_title != Some(None)
                    })
                })
                .collect::<Vec<_>>();
            let first = matching.first()?;
            let label = first
                .hotkey_overlay_title
                .as_ref()
                .and_then(Clone::clone)
                .unwrap_or_else(|| command_label(commands));
            Some(Entry {
                keys: matching.iter().map(|bind| bind.key).collect(),
                label,
            })
        })
        .collect::<Vec<_>>();

    for bind in binds
        .iter()
        .filter(|bind| matches!(bind.hotkey_overlay_title, Some(Some(_))))
    {
        if !entries.iter().any(|entry| entry.keys.contains(&bind.key)) {
            entries.push(Entry {
                keys: vec![bind.key],
                label: bind.hotkey_overlay_title.clone().flatten().unwrap(),
            });
        }
    }

    for action in [
        Action::ShowHotkeyOverlay,
        Action::Quit(false),
        Action::Quit(true),
    ] {
        if let Some((key, label)) = format_bind(binds, &action) {
            if let Some(key) = key {
                entries.push(Entry {
                    keys: vec![key],
                    label,
                });
            }
        }
    }
    entries
}

fn command_label(commands: &[&str]) -> String {
    match commands {
        ["kill"] => "Close Focused Window".into(),
        ["focus left", "focus down", "focus up", "focus right"] => {
            "Focus Left / Down / Up / Right".into()
        }
        ["move left", "move down", "move up", "move right"] => {
            "Move Window Left / Down / Up / Right".into()
        }
        ["split h", "split v"] => "Split Horizontally / Vertically".into(),
        ["layout tabbed"] => "Tabbed Layout".into(),
        ["layout stacking"] => "Stacked Layout".into(),
        ["layout toggle split"] => "Toggle Split Direction".into(),
        ["focus parent"] => "Focus Parent Container".into(),
        ["fullscreen"] => "Toggle Fullscreen".into(),
        ["floating toggle"] => "Toggle Floating".into(),
        ["mode resize"] => "Resize Mode".into(),
        ["move scratchpad"] => "Hide in Scratchpad".into(),
        ["scratchpad show"] => "Show Scratchpad".into(),
        commands if commands[0].starts_with("workspace number ") => "Switch Workspace 1–10".into(),
        commands if commands[0].starts_with("move to workspace number ") => {
            "Move Window to Workspace 1–10".into()
        }
        [command] => (*command).into(),
        _ => commands.join(" / "),
    }
}

fn format_keys(keys: &[Key], mod_key: ModKey) -> String {
    let names = keys
        .iter()
        .map(|key| key_name(false, mod_key, key))
        .collect::<Vec<_>>();
    if names.len() == 1 {
        return names[0].clone();
    }

    let mut common = names[0].split(" + ").collect::<Vec<_>>();
    common.pop();
    let prefix = common.join(" + ");
    let suffixes = names
        .iter()
        .map(|name| name.rsplit(" + ").next().unwrap_or(name))
        .collect::<Vec<_>>();
    let suffixes = if suffixes == ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"] {
        "1–0".into()
    } else {
        suffixes.join(" / ")
    };
    if prefix.is_empty() {
        suffixes
    } else {
        format!("{prefix} + {suffixes}")
    }
}

fn render(
    renderer: &mut GlesRenderer,
    config: &Config,
    mod_key: ModKey,
    scale: f64,
) -> anyhow::Result<RenderedOverlay> {
    let _span = tracy_client::span!("hotkey_overlay::render");

    // let margin = MARGIN * scale;
    let padding: i32 = to_physical_precise_round(scale, PADDING);
    let line_interval: i32 = to_physical_precise_round(scale, LINE_INTERVAL);

    // FIXME: if it doesn't fit, try splitting in two columns or something.
    // let mut target_size = output_size;
    // target_size.w -= margin * 2;
    // target_size.h -= margin * 2;
    // anyhow::ensure!(target_size.w > 0 && target_size.h > 0);

    let strings = collect_entries(config)
        .into_iter()
        .map(|entry| {
            let keys = format_keys(&entry.keys, mod_key);
            (format!(" {keys} "), entry.label)
        })
        .collect::<Vec<_>>();

    let mut font = FontDescription::from_string(FONT);
    font.set_absolute_size(to_physical_precise_round(scale, font.size()));

    let surface = ImageSurface::create(cairo::Format::ARgb32, 0, 0)?;
    let cr = cairo::Context::new(&surface)?;
    let layout = pangocairo::functions::create_layout(&cr);
    layout.context().set_round_glyph_positions(false);
    layout.set_font_description(Some(&font));

    let bold = AttrList::new();
    bold.insert(AttrInt::new_weight(Weight::Bold));
    layout.set_attributes(Some(&bold));
    layout.set_text(TITLE);
    let title_size = layout.pixel_size();

    let attrs = AttrList::new();
    attrs.insert(AttrString::new_family("Monospace"));
    attrs.insert(AttrColor::new_background(12000, 12000, 12000));

    layout.set_attributes(Some(&attrs));
    let key_sizes = strings
        .iter()
        .map(|(key, _)| {
            layout.set_text(key);
            layout.pixel_size()
        })
        .collect::<Vec<_>>();

    layout.set_attributes(None);
    let action_sizes = strings
        .iter()
        .map(|(_, action)| {
            layout.set_markup(action);
            layout.pixel_size()
        })
        .collect::<Vec<_>>();

    let key_width = key_sizes.iter().map(|(w, _)| *w).max().unwrap_or(0);
    let action_width = action_sizes.iter().map(|(w, _)| *w).max().unwrap_or(0);
    let mut width = key_width + padding + action_width;

    let mut height = zip(&key_sizes, &action_sizes)
        .map(|((_, key_h), (_, act_h))| max(key_h, act_h))
        .sum::<i32>()
        + key_sizes.len().saturating_sub(1) as i32 * line_interval
        + title_size.1
        + padding;

    width += padding * 2;
    height += padding * 2;

    let surface = ImageSurface::create(cairo::Format::ARgb32, width, height)?;
    let cr = cairo::Context::new(&surface)?;
    cr.set_source_rgb(0.1, 0.1, 0.1);
    cr.paint()?;

    cr.move_to(padding.into(), padding.into());
    let layout = pangocairo::functions::create_layout(&cr);
    layout.context().set_round_glyph_positions(false);
    layout.set_font_description(Some(&font));

    cr.set_source_rgb(1., 1., 1.);

    cr.move_to(((width - title_size.0) / 2).into(), padding.into());
    layout.set_attributes(Some(&bold));
    layout.set_text(TITLE);
    pangocairo::functions::show_layout(&cr, &layout);

    cr.move_to(padding.into(), (padding + title_size.1 + padding).into());

    for ((key, action), ((_, key_h), (_, act_h))) in zip(&strings, zip(&key_sizes, &action_sizes)) {
        layout.set_attributes(Some(&attrs));
        layout.set_text(key);
        pangocairo::functions::show_layout(&cr, &layout);

        cr.rel_move_to((key_width + padding).into(), 0.);

        let (attrs, text) = match pango::parse_markup(action, '\0') {
            Ok((attrs, text, _accel)) => (Some(attrs), text),
            Err(err) => {
                warn!("error parsing markup for key {key}: {err}");
                (None, action.into())
            }
        };

        layout.set_attributes(attrs.as_ref());
        layout.set_text(&text);
        pangocairo::functions::show_layout(&cr, &layout);

        cr.rel_move_to(
            (-(key_width + padding)).into(),
            (max(key_h, act_h) + line_interval).into(),
        );
    }

    cr.move_to(0., 0.);
    cr.line_to(width.into(), 0.);
    cr.line_to(width.into(), height.into());
    cr.line_to(0., height.into());
    cr.line_to(0., 0.);
    cr.set_source_rgb(0.5, 0.8, 1.0);
    // Keep the border width even to avoid blurry edges.
    cr.set_line_width((f64::from(BORDER) / 2. * scale).round() * 2.);
    cr.stroke()?;
    drop(cr);

    let data = surface.take_data().unwrap();
    let buffer = TextureBuffer::from_memory(
        renderer,
        &data,
        Fourcc::Argb8888,
        (width, height),
        false,
        scale,
        Transform::Normal,
        Vec::new(),
    )?;

    Ok(RenderedOverlay {
        buffer: Some(buffer),
    })
}

fn action_name(action: &Action) -> String {
    match action {
        Action::SwayCommand(command) => command.clone(),
        Action::Quit(_) => String::from("Exit swayward"),
        Action::ShowHotkeyOverlay => String::from("Show Important Hotkeys"),
        Action::CloseWindow => String::from("Close Focused Window"),
        Action::FocusColumnLeft => String::from("Focus Column to the Left"),
        Action::FocusColumnRight => String::from("Focus Column to the Right"),
        Action::MoveColumnLeft => String::from("Move Column Left"),
        Action::MoveColumnRight => String::from("Move Column Right"),
        Action::FocusWorkspaceDown => String::from("Switch Workspace Down"),
        Action::FocusWorkspaceUp => String::from("Switch Workspace Up"),
        Action::MoveColumnToWorkspaceDown(_) => String::from("Move Column to Workspace Down"),
        Action::MoveColumnToWorkspaceUp(_) => String::from("Move Column to Workspace Up"),
        Action::MoveWindowToWorkspaceDown(_) => String::from("Move Window to Workspace Down"),
        Action::MoveWindowToWorkspaceUp(_) => String::from("Move Window to Workspace Up"),
        Action::SwitchPresetColumnWidth => String::from("Switch Preset Column Widths"),
        Action::MaximizeColumn => String::from("Maximize Column"),
        Action::ConsumeOrExpelWindowLeft => String::from("Consume or Expel Window Left"),
        Action::ConsumeOrExpelWindowRight => String::from("Consume or Expel Window Right"),
        Action::ToggleWindowFloating => String::from("Move Window Between Floating and Tiling"),
        Action::SwitchFocusBetweenFloatingAndTiling => {
            String::from("Switch Focus Between Floating and Tiling")
        }
        Action::ToggleOverview => String::from("Open the Overview"),
        Action::Screenshot(_, _) => String::from("Take a Screenshot"),
        Action::Spawn(args) => format!(
            "Spawn <span face='monospace' bgcolor='#000000'>{}</span>",
            args.first().unwrap_or(&String::new())
        ),
        Action::SpawnSh(command) => format!(
            "Spawn <span face='monospace' bgcolor='#000000'>{}</span>",
            // Fairly crude but should get the job done in most cases.
            command.split_ascii_whitespace().next().unwrap_or("")
        ),
        _ => String::from("FIXME: Unknown"),
    }
}

fn key_name(screen_reader: bool, mod_key: ModKey, key: &Key) -> String {
    let mut name = String::new();

    let has_comp_mod = key.modifiers.contains(Modifiers::COMPOSITOR);

    // Compositor mod goes first.
    if has_comp_mod {
        match mod_key {
            ModKey::None => {}
            ModKey::Super => {
                name.push_str("Super + ");
            }
            ModKey::Alt => {
                name.push_str("Alt + ");
            }
            ModKey::Shift => {
                name.push_str("Shift + ");
            }
            ModKey::Ctrl => {
                name.push_str("Ctrl + ");
            }
            ModKey::IsoLevel3Shift => {
                name.push_str("Mod5 + ");
            }
            ModKey::IsoLevel5Shift => {
                name.push_str("Mod3 + ");
            }
        }
    }

    if key.modifiers.contains(Modifiers::SUPER) && !(has_comp_mod && mod_key == ModKey::Super) {
        name.push_str("Super + ");
    }
    if key.modifiers.contains(Modifiers::CTRL) && !(has_comp_mod && mod_key == ModKey::Ctrl) {
        name.push_str("Ctrl + ");
    }
    if key.modifiers.contains(Modifiers::SHIFT) && !(has_comp_mod && mod_key == ModKey::Shift) {
        name.push_str("Shift + ");
    }
    if key.modifiers.contains(Modifiers::ALT) && !(has_comp_mod && mod_key == ModKey::Alt) {
        name.push_str("Alt + ");
    }
    if key.modifiers.contains(Modifiers::NUM) {
        name.push_str("Num + ");
    }
    if key.modifiers.contains(Modifiers::CAPS) {
        name.push_str("Caps Lock + ");
    }
    if key.modifiers.contains(Modifiers::ISO_LEVEL3_SHIFT)
        && !(has_comp_mod && mod_key == ModKey::IsoLevel3Shift)
    {
        name.push_str("Mod5 + ");
    }
    if key.modifiers.contains(Modifiers::ISO_LEVEL5_SHIFT)
        && !(has_comp_mod && mod_key == ModKey::IsoLevel5Shift)
    {
        name.push_str("Mod3 + ");
    }

    let pretty = match key.trigger {
        Trigger::Keysym(keysym) => prettify_keysym_name(screen_reader, &keysym_get_name(keysym)),
        Trigger::Keycode(keycode) => format!("code:{keycode}"),
        Trigger::MouseLeft => String::from("Mouse Left"),
        Trigger::MouseRight => String::from("Mouse Right"),
        Trigger::MouseMiddle => String::from("Mouse Middle"),
        Trigger::MouseBack => String::from("Mouse Back"),
        Trigger::MouseForward => String::from("Mouse Forward"),
        Trigger::WheelScrollDown => String::from("Wheel Scroll Down"),
        Trigger::WheelScrollUp => String::from("Wheel Scroll Up"),
        Trigger::WheelScrollLeft => String::from("Wheel Scroll Left"),
        Trigger::WheelScrollRight => String::from("Wheel Scroll Right"),
        Trigger::TouchpadScrollDown => String::from("Touchpad Scroll Down"),
        Trigger::TouchpadScrollUp => String::from("Touchpad Scroll Up"),
        Trigger::TouchpadScrollLeft => String::from("Touchpad Scroll Left"),
        Trigger::TouchpadScrollRight => String::from("Touchpad Scroll Right"),
        Trigger::TabletStylusButton1 => String::from("Tablet Stylus Button 1"),
        Trigger::TabletStylusButton2 => String::from("Tablet Stylus Button 2"),
        Trigger::TabletStylusButton3 => String::from("Tablet Stylus Button 3"),
    };
    name.push_str(&pretty);

    name
}

fn prettify_keysym_name(screen_reader: bool, name: &str) -> String {
    let name = if screen_reader {
        name
    } else {
        match name {
            "slash" => "/",
            "comma" => ",",
            "period" => ".",
            "minus" => "-",
            "equal" => "=",
            "grave" => "`",
            "bracketleft" => "[",
            "bracketright" => "]",
            "adiaeresis" => "Ä",
            "ediaeresis" => "Ë",
            "idiaeresis" => "Ï",
            "odiaeresis" => "Ö",
            "udiaeresis" => "Ü",
            "ydiaeresis" => "Ÿ",
            "wdiaeresis" => "Ẅ",
            _ => name,
        }
    };

    let name = match name {
        "Next" => "Page Down",
        "Prior" => "Page Up",
        "Print" => "PrtSc",
        "Return" => "Enter",
        "space" => "Space",
        _ => name,
    };

    if name.len() == 1 && name.is_ascii() {
        name.to_ascii_uppercase()
    } else {
        name.into()
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use super::*;

    #[track_caller]
    fn check(config: &str, action: Action) -> String {
        let config = Config::parse_mem(config).unwrap();
        if let Some((key, title)) = format_bind(&config.binds.0, &action) {
            let key = key.map(|key| key_name(false, ModKey::Super, &key));
            let key = key.as_deref().unwrap_or("(not bound)");
            format!(" {key} : {title}")
        } else {
            String::from("None")
        }
    }

    #[test]
    fn key_name_includes_lock_modifiers() {
        let key = "Lock+Num+Q".parse().unwrap();
        assert_eq!(key_name(false, ModKey::None, &key), "Num + Caps Lock + Q");
    }

    #[test]
    fn default_overlay_entries_are_bound_and_use_sway_vocabulary() {
        let config = Config::load_default();
        let entries = collect_entries(&config);
        assert!(entries.iter().all(|entry| {
            entry.keys.iter().all(|key| {
                config
                    .binds
                    .0
                    .iter()
                    .any(|bind| bind.key == *key && bind.hotkey_overlay_title != Some(None))
            })
        }));
        assert!(config.binds.0.iter().all(|bind| {
            !matches!(bind.hotkey_overlay_title, Some(Some(_)))
                || entries.iter().any(|entry| entry.keys.contains(&bind.key))
        }));
        assert!(entries.iter().any(|entry| entry.label == "Tabbed Layout"));
        assert!(entries.iter().any(|entry| entry.label == "Resize Mode"));
        assert!(!entries.iter().any(|entry| entry.label.contains("Column")));
    }

    #[test]
    fn empty_config_does_not_advertise_unbound_hotkeys() {
        assert!(collect_entries(&Config::parse_mem("").unwrap()).is_empty());
    }

    #[test]
    fn unlisted_sway_command_uses_its_raw_string() {
        assert_eq!(command_label(&["frobnicate"]), "frobnicate");
    }

    #[test]
    fn test_format_bind() {
        // Not bound.
        assert_snapshot!(check("", Action::Screenshot(true, None)), @" (not bound) : Take a Screenshot");

        // Bound with a default title.
        assert_snapshot!(
            check(
                r#"binds {
                    Mod+P { screenshot; }
                }"#,
                Action::Screenshot(true, None),
            ),
            @" Super + P : Take a Screenshot"
        );

        // Custom title.
        assert_snapshot!(
            check(
                r#"binds {
                    Mod+P hotkey-overlay-title="Hello" { screenshot; }
                }"#,
                Action::Screenshot(true, None),
            ),
            @" Super + P : Hello"
        );

        // Prefer first bind.
        assert_snapshot!(
            check(
                r#"binds {
                    Mod+P { screenshot; }
                    Print { screenshot; }
                }"#,
                Action::Screenshot(true, None),
            ),
            @" Super + P : Take a Screenshot"
        );

        // Prefer bind with custom title.
        assert_snapshot!(
            check(
                r#"binds {
                    Mod+P { screenshot; }
                    Print hotkey-overlay-title="My Cool Bind" { screenshot; }
                }"#,
                Action::Screenshot(true, None),
            ),
            @" PrtSc : My Cool Bind"
        );

        // Prefer first bind with custom title.
        assert_snapshot!(
            check(
                r#"binds {
                    Mod+P hotkey-overlay-title="First" { screenshot; }
                    Print hotkey-overlay-title="My Cool Bind" { screenshot; }
                }"#,
                Action::Screenshot(true, None),
            ),
            @" Super + P : First"
        );

        // Any bind with null title hides it.
        assert_snapshot!(
            check(
                r#"binds {
                    Mod+P { screenshot; }
                    Print hotkey-overlay-title=null { screenshot; }
                }"#,
                Action::Screenshot(true, None),
            ),
            @"None"
        );

        // Custom title takes preference over null.
        assert_snapshot!(
            check(
                r#"binds {
                    Mod+P hotkey-overlay-title="Hello" { screenshot; }
                    Print hotkey-overlay-title=null { screenshot; }
                }"#,
                Action::Screenshot(true, None),
            ),
            @" Super + P : Hello"
        );
    }
}
