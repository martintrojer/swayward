use smithay::input::keyboard::XkbConfig;

use crate::utils::{Flag, MergeWith};

#[derive(Debug, PartialEq, Eq)]
pub struct Keyboard {
    pub xkb: Xkb,
    pub repeat_delay: u16,
    pub repeat_rate: u8,
    pub track_layout: TrackLayout,
    pub numlock: bool,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            xkb: Default::default(),
            // The defaults were chosen to match wlroots and sway.
            repeat_delay: 600,
            repeat_rate: 25,
            track_layout: Default::default(),
            numlock: Default::default(),
        }
    }
}

#[derive(knuffel::Decode, Debug, PartialEq, Eq)]
pub struct KeyboardPart {
    #[knuffel(child)]
    pub xkb: Option<Xkb>,
    #[knuffel(child, unwrap(argument))]
    pub repeat_delay: Option<u16>,
    #[knuffel(child, unwrap(argument))]
    pub repeat_rate: Option<u8>,
    #[knuffel(child, unwrap(argument))]
    pub track_layout: Option<TrackLayout>,
    #[knuffel(child)]
    pub numlock: Option<Flag>,
}

impl MergeWith<KeyboardPart> for Keyboard {
    fn merge_with(&mut self, part: &KeyboardPart) {
        merge_clone!((self, part), xkb, repeat_delay, repeat_rate, track_layout);
        merge!((self, part), numlock);
    }
}

#[derive(knuffel::Decode, Debug, Default, PartialEq, Eq, Clone)]
pub struct Xkb {
    #[knuffel(child, unwrap(argument), default)]
    pub rules: String,
    #[knuffel(child, unwrap(argument), default)]
    pub model: String,
    #[knuffel(child, unwrap(argument), default)]
    pub layout: String,
    #[knuffel(child, unwrap(argument), default)]
    pub variant: String,
    #[knuffel(child, unwrap(argument))]
    pub options: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub file: Option<String>,
}

impl Xkb {
    pub fn to_xkb_config(&self) -> XkbConfig<'_> {
        XkbConfig {
            rules: &self.rules,
            model: &self.model,
            layout: &self.layout,
            variant: &self.variant,
            options: self.options.clone(),
        }
    }
}

#[derive(knuffel::DecodeScalar, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TrackLayout {
    /// The layout change is global.
    #[default]
    Global,
    /// The layout change is window local.
    Window,
}
