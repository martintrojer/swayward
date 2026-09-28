use crate::utils::MergeWith;
use crate::FloatOrInt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blur {
    pub off: bool,
    pub passes: u8,
    pub offset: f64,
    pub noise: f64,
    pub saturation: f64,
}

impl Default for Blur {
    fn default() -> Self {
        Self {
            off: false,
            passes: 3,
            offset: 3.,
            noise: 0.02,
            saturation: 1.5,
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct BlurPart {
    #[knuffel(child)]
    pub off: bool,
    #[knuffel(child)]
    pub on: bool,
    #[knuffel(child, unwrap(argument))]
    pub passes: Option<u8>,
    #[knuffel(child, unwrap(argument))]
    pub offset: Option<FloatOrInt<0, 100>>,
    #[knuffel(child, unwrap(argument))]
    pub noise: Option<FloatOrInt<0, 1000>>,
    #[knuffel(child, unwrap(argument))]
    pub saturation: Option<FloatOrInt<0, 1000>>,
}

impl MergeWith<BlurPart> for Blur {
    fn merge_with(&mut self, part: &BlurPart) {
        self.off |= part.off;
        if part.on {
            self.off = false;
        }

        merge_clone!((self, part), passes);
        merge!((self, part), offset, noise, saturation);
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct BackgroundEffectRule {
    #[knuffel(child, unwrap(argument))]
    pub xray: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub blur: Option<bool>,
    #[knuffel(child, unwrap(argument))]
    pub noise: Option<FloatOrInt<0, 1000>>,
    #[knuffel(child, unwrap(argument))]
    pub saturation: Option<FloatOrInt<0, 1000>>,
}

/// Resolved background effect rule.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct BackgroundEffect {
    /// Whether to render with xray effect (see through).
    ///
    /// - `None`: xray if any background effect is active
    /// - `Some(false)`: no xray
    /// - `Some(true)`: xray even if no other background effect is active
    pub xray: Option<bool>,

    /// Whether to blur the background.
    ///
    /// - `None`: blur when the window/layer requests it (e.g. through ext-background-effect
    ///   protocol)
    /// - `Some(false)`: never blur
    /// - `Some(true)`: always blur
    pub blur: Option<bool>,

    pub noise: Option<f64>,
    pub saturation: Option<f64>,
}

impl MergeWith<BackgroundEffectRule> for BackgroundEffect {
    fn merge_with(&mut self, part: &BackgroundEffectRule) {
        merge_clone_opt!((self, part), xray, blur);

        if let Some(x) = part.noise {
            self.noise = Some(x.0);
        }

        if let Some(x) = part.saturation {
            self.saturation = Some(x.0);
        }
    }
}
