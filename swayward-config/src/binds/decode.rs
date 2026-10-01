use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::time::Duration;

use knuffel::errors::DecodeError;

use super::{Action, Bind, BindIdentity, Binds, Key, MouseRegions, Trigger};
use crate::utils::expect_only_children;

impl Binds {
    pub(crate) fn merge(&mut self, part: Self) {
        self.0
            .retain(|bind| !part.0.iter().any(|new| new.conflicts_with(bind)));
        self.0.extend(part.0);
    }

    pub(super) fn decode_children<S: knuffel::traits::ErrorSpan>(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Self {
        let mut seen_keys: HashMap<BindIdentity, &knuffel::ast::SpannedNode<S>> = HashMap::new();
        let mut binds = Vec::new();

        for child in node.children() {
            match <Bind as knuffel::Decode<S>>::decode_node(child, ctx) {
                Err(e) => ctx.emit_error(e),
                Ok(bind) => match seen_keys.entry(bind.identity()) {
                    Entry::Occupied(entry) => {
                        // Even though it's technically incorrect, we use
                        // `DecodeError::Missing` here because it labels the bind with
                        // "node starts here", which is the least bad option
                        ctx.emit_error(DecodeError::missing(
                            entry.get(),
                            "keybind first defined here",
                        ));
                        ctx.emit_error(DecodeError::unexpected(
                            &child.node_name,
                            "keybind",
                            "duplicate keybind later defined here",
                        ));
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(child);
                        binds.push(bind);
                    }
                },
            }
        }
        Self(binds)
    }
}

impl<S> knuffel::Decode<S> for Binds
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        expect_only_children(node, ctx);
        Ok(Self::decode_children(node, ctx))
    }
}

impl<S> knuffel::Decode<S> for Bind
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        if let Some(type_name) = &node.type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }

        for val in node.arguments.iter() {
            ctx.emit_error(DecodeError::unexpected(
                &val.literal,
                "argument",
                "no arguments expected for this node",
            ));
        }

        let mut key_name = node.node_name.to_string();
        let mut group = None;
        let mut key_parts = key_name.split('+').collect::<Vec<_>>();
        key_parts.retain(|part| {
            let value = if *part == "Mode_switch" {
                Some("2")
            } else {
                part.strip_prefix("Group")
            };
            let Some(value) = value else {
                return true;
            };
            match value.parse::<u8>() {
                Ok(value @ 1..=4) if group.is_none() => group = Some(value - 1),
                _ => ctx.emit_error(DecodeError::unexpected(
                    &node.node_name,
                    "keybind",
                    "exactly one XKB group from Group1 to Group4 is allowed",
                )),
            }
            false
        });
        key_name = key_parts.join("+");
        let key = key_name
            .parse::<Key>()
            .map_err(|e| DecodeError::conversion(&node.node_name, e.wrap_err("invalid keybind")))?;

        let mut mouse_regions = MouseRegions::empty();
        let mut input_device = "*".to_owned();
        let mut release = false;
        let mut repeat = true;
        let mut cooldown = None;
        let mut allow_when_locked = false;
        let mut allow_when_locked_node = None;
        let mut allow_inhibiting = true;
        let mut hotkey_overlay_title = None;
        for (name, val) in &node.properties {
            match &***name {
                "mouse-regions" => {
                    let regions: String = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                    for region in regions.split('+') {
                        mouse_regions |= match region {
                            "titlebar" => MouseRegions::TITLEBAR,
                            "border" => MouseRegions::BORDER,
                            "contents" => MouseRegions::CONTENTS,
                            _ => {
                                ctx.emit_error(DecodeError::unexpected(
                                    name,
                                    "property",
                                    "mouse-regions must contain titlebar, border, or contents",
                                ));
                                MouseRegions::empty()
                            }
                        };
                    }
                }
                "input-device" => {
                    input_device = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                    if input_device.is_empty() {
                        ctx.emit_error(DecodeError::unexpected(
                            &val.literal,
                            "property value",
                            "input-device must not be empty",
                        ));
                    }
                }
                "release" => {
                    release = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                }
                "repeat" => {
                    repeat = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                }
                "cooldown-ms" => {
                    cooldown = Some(Duration::from_millis(
                        knuffel::traits::DecodeScalar::decode(val, ctx)?,
                    ));
                }
                "allow-when-locked" => {
                    allow_when_locked = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                    allow_when_locked_node = Some(name);
                }
                "allow-inhibiting" => {
                    allow_inhibiting = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                }
                "hotkey-overlay-title" => {
                    hotkey_overlay_title = Some(knuffel::traits::DecodeScalar::decode(val, ctx)?);
                }
                name_str => {
                    ctx.emit_error(DecodeError::unexpected(
                        name,
                        "property",
                        format!("unexpected property `{}`", name_str.escape_default()),
                    ));
                }
            }
        }

        let keyboard_trigger = matches!(key.trigger, Trigger::Keysym(_) | Trigger::Keycode(_));
        if keyboard_trigger && !mouse_regions.is_empty() {
            ctx.emit_error(DecodeError::unexpected(
                &node.node_name,
                "keybind",
                "mouse-regions requires a pointer trigger",
            ));
        }
        if !keyboard_trigger && group.is_some() {
            ctx.emit_error(DecodeError::unexpected(
                &node.node_name,
                "keybind",
                "XKB groups require a keyboard trigger",
            ));
        }

        if release {
            repeat = false;
        }

        let mut children = node.children();

        // If the action is invalid but the key is fine, we still want to return something.
        // That way, the parent can handle the existence of duplicate keybinds,
        // even if their contents are not valid.
        let dummy = Self {
            key,
            action: Action::Spawn(vec![]),
            mouse_regions,
            input_device: input_device.clone(),
            group,
            release,
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        };

        if let Some(child) = children.next() {
            for unwanted_child in children {
                ctx.emit_error(DecodeError::unexpected(
                    unwanted_child,
                    "node",
                    "only one action is allowed per keybind",
                ));
            }
            if child.node_name.as_ref() == "command" {
                let command = match &child.arguments[..] {
                    [argument] => match &*argument.literal {
                        knuffel::ast::Literal::String(command) if !command.trim().is_empty() => {
                            command.to_string()
                        }
                        knuffel::ast::Literal::String(_) => {
                            ctx.emit_error(DecodeError::unexpected(
                                &argument.literal,
                                "argument",
                                "command must not be empty",
                            ));
                            return Ok(dummy);
                        }
                        _ => {
                            ctx.emit_error(DecodeError::unexpected(
                                &argument.literal,
                                "argument",
                                "command must be a quoted string",
                            ));
                            return Ok(dummy);
                        }
                    },
                    _ => {
                        ctx.emit_error(DecodeError::unexpected(
                            child,
                            "node",
                            "expected command \"<sway command>\"",
                        ));
                        return Ok(dummy);
                    }
                };
                if child.children.is_some() || !child.properties.is_empty() {
                    ctx.emit_error(DecodeError::unexpected(
                        child,
                        "node",
                        "command accepts one quoted string and no children or properties",
                    ));
                    return Ok(dummy);
                }
                return Ok(Self {
                    key,
                    action: Action::SwayCommand(command),
                    mouse_regions,
                    input_device,
                    group,
                    release,
                    repeat,
                    cooldown,
                    allow_when_locked,
                    allow_inhibiting,
                    hotkey_overlay_title,
                });
            }
            match Action::decode_node(child, ctx) {
                Ok(action) => {
                    if !matches!(action, Action::Spawn(_) | Action::SpawnSh(_)) {
                        if let Some(node) = allow_when_locked_node {
                            ctx.emit_error(DecodeError::unexpected(
                                node,
                                "property",
                                "allow-when-locked can only be set on spawn binds",
                            ));
                        }
                    }

                    // The toggle-inhibit action must always be uninhibitable.
                    // Otherwise, it would be impossible to trigger it.
                    if matches!(action, Action::ToggleKeyboardShortcutsInhibit) {
                        allow_inhibiting = false;
                    }

                    Ok(Self {
                        key,
                        action,
                        mouse_regions,
                        input_device,
                        group,
                        release,
                        repeat,
                        cooldown,
                        allow_when_locked,
                        allow_inhibiting,
                        hotkey_overlay_title,
                    })
                }
                Err(e) => {
                    ctx.emit_error(e);
                    Ok(dummy)
                }
            }
        } else {
            ctx.emit_error(DecodeError::missing(
                node,
                "expected an action for this keybind",
            ));
            Ok(dummy)
        }
    }
}
