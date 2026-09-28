use super::*;

pub(super) fn rect_from(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x: x.round() as i32,
        y: y.round() as i32,
        width: width.round() as i32,
        height: height.round() as i32,
    }
}
pub(super) fn offset_rect(rect: Rectangle<f64, Logical>, output: Rect) -> Rect {
    rect_from(
        rect.loc.x + f64::from(output.x),
        rect.loc.y + f64::from(output.y),
        rect.size.w,
        rect.size.h,
    )
}
pub(super) fn rect_from_rectangle(rect: Rectangle<i32, Logical>) -> Rect {
    Rect {
        x: rect.loc.x,
        y: rect.loc.y,
        width: rect.size.w,
        height: rect.size.h,
    }
}

pub(super) fn output_rect(global_space: &Space<Window>, output: &smithay::output::Output) -> Rect {
    global_space
        .output_geometry(output)
        .map(rect_from_rectangle)
        .unwrap_or_default()
}

/// The output's usable area, in global coordinates.
///
/// This is the output rect minus layer-shell exclusive zones and the
/// workspace's effective outer gaps. Sway includes the edge half of the inner
/// gap in this inset as well.
pub(super) fn workspace_rect(
    global_space: &Space<Window>,
    output: &smithay::output::Output,
    workspace: &crate::layout::workspace::Workspace<Mapped>,
) -> Rect {
    let Some(geometry) = global_space.output_geometry(output) else {
        return Rect::default();
    };
    let area = workspace.working_area();
    rect_from_rectangle(Rectangle::new(geometry.loc.to_f64() + area.loc, area.size).to_i32_round())
}
