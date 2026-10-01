//! Outputs: connect, disconnect, focus and per-output layout config.

use super::*;

pub(super) fn apply(op: Op, layout: &mut Layout<TestWindow>) -> Applied {
    match op {
        Op::AddOutput(id) => {
            let name = format!("output{id}");
            if layout.outputs().any(|o| o.name() == name) {
                return Applied::Done;
            }

            let output = Output::new(
                name.clone(),
                PhysicalProperties {
                    size: Size::from((1280, 720)),
                    subpixel: Subpixel::Unknown,
                    make: String::new(),
                    model: String::new(),
                    serial_number: String::new(),
                },
            );
            output.change_current_state(
                Some(Mode {
                    size: Size::from((1280, 720)),
                    refresh: 60000,
                }),
                None,
                None,
                None,
            );
            output.user_data().insert_if_missing(|| OutputName {
                connector: name,
                make: None,
                model: None,
                serial: None,
            });
            layout.add_output(output.clone(), None);
        }
        Op::AddScaledOutput {
            id,
            scale,
            layout_config,
        } => {
            let name = format!("output{id}");
            if layout.outputs().any(|o| o.name() == name) {
                return Applied::Done;
            }

            let output = Output::new(
                name.clone(),
                PhysicalProperties {
                    size: Size::from((1280, 720)),
                    subpixel: Subpixel::Unknown,
                    make: String::new(),
                    model: String::new(),
                    serial_number: String::new(),
                },
            );
            output.change_current_state(
                Some(Mode {
                    size: Size::from((1280, 720)),
                    refresh: 60000,
                }),
                None,
                Some(smithay::output::Scale::Fractional(scale)),
                None,
            );
            output.user_data().insert_if_missing(|| OutputName {
                connector: name,
                make: None,
                model: None,
                serial: None,
            });
            layout.add_output(output.clone(), layout_config.map(|x| *x));
        }
        Op::RemoveOutput(id) => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.remove_output(&output);
        }
        Op::FocusOutput(id) => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.focus_output(&output);
        }
        Op::UpdateOutputLayoutConfig { id, layout_config } => {
            let name = format!("output{id}");
            let Some(mon) = layout.monitors_mut().find(|m| m.output_name() == &name) else {
                return Applied::Done;
            };

            mon.update_layout_config(layout_config.map(|x| *x));
        }
        other => return Applied::NotMine(Box::new(other)),
    }
    Applied::Done
}
