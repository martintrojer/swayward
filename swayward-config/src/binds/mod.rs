use std::time::Duration;

use knuffel::errors::DecodeError;

use crate::utils::MergeWith;

mod action;
mod decode;
mod key;

pub use action::{Action, WorkspaceReference};
pub use key::{Key, Modifiers, MouseRegions, Trigger};

#[derive(Debug, Default, PartialEq)]
pub struct Binds(pub Vec<Bind>);

#[derive(Debug, PartialEq)]
pub struct BindingMode {
    pub name: String,
    pub pango_markup: bool,
    pub binds: Binds,
}

impl<S> knuffel::Decode<S> for BindingMode
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let name = match &node.arguments[..] {
            [argument] => knuffel::traits::DecodeScalar::decode(argument, ctx)?,
            _ => {
                return Err(DecodeError::unexpected(
                    node,
                    "mode",
                    "expected mode \"<name>\" { ... }",
                ));
            }
        };
        let mut pango_markup = false;
        for (property, value) in &node.properties {
            if &***property == "pango-markup" {
                pango_markup = knuffel::traits::DecodeScalar::decode(value, ctx)?;
            } else {
                ctx.emit_error(DecodeError::unexpected(
                    property,
                    "property",
                    "only pango-markup is expected for mode",
                ));
            }
        }
        Ok(Self {
            name,
            pango_markup,
            binds: Binds::decode_children(node, ctx),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bind {
    pub key: Key,
    pub action: Action,
    pub mouse_regions: MouseRegions,
    /// Sway-compatible input identifier, or `"*"` to match every device.
    pub input_device: String,
    /// Zero-based XKB layout group. `None` matches every active group.
    pub group: Option<u8>,
    pub release: bool,
    pub repeat: bool,
    pub cooldown: Option<Duration>,
    pub allow_when_locked: bool,
    pub allow_inhibiting: bool,
    pub hotkey_overlay_title: Option<Option<String>>,
}

/// The fields that make two bindings the same binding: a later one with an
/// equal identity replaces the earlier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BindIdentity {
    pub key: Key,
    pub mouse_regions: MouseRegions,
    pub input_device: String,
    pub group: Option<u8>,
    pub release: bool,
    pub allow_when_locked: bool,
    pub allow_inhibiting: bool,
}

impl Bind {
    pub fn identity(&self) -> BindIdentity {
        BindIdentity {
            key: self.key,
            mouse_regions: self.mouse_regions,
            input_device: self.input_device.clone(),
            group: self.group,
            release: self.release,
            allow_when_locked: self.allow_when_locked,
            allow_inhibiting: self.allow_inhibiting,
        }
    }

    /// The identity a cooldown is tracked under. Bindings that differ only in
    /// mouse region share one cooldown.
    pub fn cooldown_identity(&self) -> BindIdentity {
        BindIdentity {
            mouse_regions: MouseRegions::empty(),
            ..self.identity()
        }
    }

    pub(crate) fn conflicts_with(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct SwitchBinds {
    #[knuffel(child)]
    pub lid_open: Option<SwitchAction>,
    #[knuffel(child)]
    pub lid_close: Option<SwitchAction>,
    #[knuffel(child)]
    pub tablet_mode_on: Option<SwitchAction>,
    #[knuffel(child)]
    pub tablet_mode_off: Option<SwitchAction>,
}

impl MergeWith<SwitchBinds> for SwitchBinds {
    fn merge_with(&mut self, part: &SwitchBinds) {
        merge_clone_opt!(
            (self, part),
            lid_open,
            lid_close,
            tablet_mode_on,
            tablet_mode_off,
        );
    }
}

#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub struct SwitchAction {
    #[knuffel(child, unwrap(arguments))]
    pub spawn: Vec<String>,
}
