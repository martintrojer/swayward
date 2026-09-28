use std::str::FromStr;

use miette::miette;

use super::Color;
use crate::utils::{Flag, MergeWith};
use crate::FloatOrInt;

#[derive(Debug, Clone, PartialEq)]
pub struct Titlebar {
    pub font: String,
    pub pango_markup: bool,
    pub show_marks: bool,
    pub alignment: TitleAlignment,
    pub horizontal_padding: f64,
    pub vertical_padding: f64,
    pub border_thickness: u16,
    pub focused: TitlebarColors,
    pub focused_inactive: TitlebarColors,
    pub focused_tab_title: TitlebarColors,
    pub unfocused: TitlebarColors,
    pub urgent: TitlebarColors,
}

impl Default for Titlebar {
    fn default() -> Self {
        // sway/sway/config.c initializes these five client color fields.
        let focused = TitlebarColors {
            border_color: Color::from_rgba8_unpremul(0x4c, 0x78, 0x99, 0xff),
            background_color: Color::from_rgba8_unpremul(0x28, 0x55, 0x77, 0xff),
            text_color: Color::from_rgba8_unpremul(0xff, 0xff, 0xff, 0xff),
        };
        let focused_inactive = TitlebarColors {
            border_color: Color::from_rgba8_unpremul(0x33, 0x33, 0x33, 0xff),
            background_color: Color::from_rgba8_unpremul(0x5f, 0x67, 0x6a, 0xff),
            text_color: Color::from_rgba8_unpremul(0xff, 0xff, 0xff, 0xff),
        };
        let unfocused = TitlebarColors {
            border_color: Color::from_rgba8_unpremul(0x33, 0x33, 0x33, 0xff),
            background_color: Color::from_rgba8_unpremul(0x22, 0x22, 0x22, 0xff),
            text_color: Color::from_rgba8_unpremul(0x88, 0x88, 0x88, 0xff),
        };
        Self {
            font: "monospace 10".to_owned(),
            pango_markup: false,
            show_marks: true,
            alignment: TitleAlignment::Left,
            horizontal_padding: 5.,
            vertical_padding: 4.,
            border_thickness: 1,
            focused,
            focused_inactive,
            focused_tab_title: focused_inactive,
            unfocused,
            urgent: TitlebarColors {
                border_color: Color::from_rgba8_unpremul(0x2f, 0x34, 0x3a, 0xff),
                background_color: Color::from_rgba8_unpremul(0x90, 0, 0, 0xff),
                text_color: Color::from_rgba8_unpremul(0xff, 0xff, 0xff, 0xff),
            },
        }
    }
}

impl MergeWith<TitlebarPart> for Titlebar {
    fn merge_with(&mut self, part: &TitlebarPart) {
        merge_clone!((self, part), font, pango_markup, alignment);
        merge!(
            (self, part),
            show_marks,
            horizontal_padding,
            vertical_padding,
        );
        merge_clone!((self, part), border_thickness);
        if let Some(colors) = &part.focused {
            self.focused.merge_with(colors);
        }
        if let Some(colors) = &part.focused_inactive {
            self.focused_inactive.merge_with(colors);
        }
        if let Some(colors) = &part.focused_tab_title {
            self.focused_tab_title.merge_with(colors);
        }
        if let Some(colors) = &part.unfocused {
            self.unfocused.merge_with(colors);
        }
        if let Some(colors) = &part.urgent {
            self.urgent.merge_with(colors);
        }
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct TitlebarPart {
    #[knuffel(child, unwrap(argument, str))]
    pub font: Option<String>,
    #[knuffel(child, unwrap(argument))]
    pub pango_markup: Option<bool>,
    #[knuffel(child)]
    pub show_marks: Option<Flag>,
    #[knuffel(child, unwrap(argument, str))]
    pub alignment: Option<TitleAlignment>,
    #[knuffel(child, unwrap(argument))]
    pub horizontal_padding: Option<FloatOrInt<0, 65535>>,
    #[knuffel(child, unwrap(argument))]
    pub vertical_padding: Option<FloatOrInt<0, 65535>>,
    #[knuffel(child, unwrap(argument))]
    pub border_thickness: Option<u16>,
    #[knuffel(child)]
    pub focused: Option<TitlebarColorsPart>,
    #[knuffel(child)]
    pub focused_inactive: Option<TitlebarColorsPart>,
    #[knuffel(child)]
    pub focused_tab_title: Option<TitlebarColorsPart>,
    #[knuffel(child)]
    pub unfocused: Option<TitlebarColorsPart>,
    #[knuffel(child)]
    pub urgent: Option<TitlebarColorsPart>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TitleAlignment {
    #[default]
    Left,
    Center,
    Right,
}

impl FromStr for TitleAlignment {
    type Err = miette::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "left" => Ok(Self::Left),
            "center" => Ok(Self::Center),
            "right" => Ok(Self::Right),
            _ => Err(miette!("unknown title alignment `{value}`")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TitlebarColors {
    pub border_color: Color,
    pub background_color: Color,
    pub text_color: Color,
}

impl MergeWith<TitlebarColorsPart> for TitlebarColors {
    fn merge_with(&mut self, part: &TitlebarColorsPart) {
        merge_clone!((self, part), border_color, background_color, text_color);
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, Copy, PartialEq)]
pub struct TitlebarColorsPart {
    #[knuffel(child)]
    pub border_color: Option<Color>,
    #[knuffel(child)]
    pub background_color: Option<Color>,
    #[knuffel(child)]
    pub text_color: Option<Color>,
}
