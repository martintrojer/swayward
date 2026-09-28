#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct Tablet {
    #[knuffel(child)]
    pub off: bool,
    #[knuffel(child, unwrap(arguments))]
    pub calibration_matrix: Option<Vec<f32>>,
    #[knuffel(child, unwrap(argument))]
    pub map_to_output: Option<String>,
    #[knuffel(child)]
    pub map_to_focused_output: bool,
    #[knuffel(child)]
    pub map_to_focused_window: bool,
    #[knuffel(child)]
    pub left_handed: bool,
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct Touch {
    #[knuffel(child)]
    pub off: bool,
    #[knuffel(child, unwrap(arguments))]
    pub calibration_matrix: Option<Vec<f32>>,
    #[knuffel(child, unwrap(argument))]
    pub map_to_output: Option<String>,
}
