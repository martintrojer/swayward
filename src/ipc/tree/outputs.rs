use super::workspaces::WorkspaceNodeContext;
use super::*;

pub(crate) fn sway_transform(transform: smithay::utils::Transform) -> &'static str {
    match transform {
        // Sway reports clockwise rotations; Smithay's transform is
        // counter-clockwise, so 90 and 270 are inverted.
        smithay::utils::Transform::Normal => "normal",
        smithay::utils::Transform::_90 => "270",
        smithay::utils::Transform::_180 => "180",
        smithay::utils::Transform::_270 => "90",
        smithay::utils::Transform::Flipped => "flipped",
        smithay::utils::Transform::Flipped90 => "flipped-270",
        smithay::utils::Transform::Flipped180 => "flipped-180",
        smithay::utils::Transform::Flipped270 => "flipped-90",
    }
}

pub(crate) fn sway_subpixel_hinting(subpixel: smithay::output::Subpixel) -> &'static str {
    match subpixel {
        smithay::output::Subpixel::Unknown => "unknown",
        smithay::output::Subpixel::None => "none",
        smithay::output::Subpixel::HorizontalRgb => "rgb",
        smithay::output::Subpixel::HorizontalBgr => "bgr",
        smithay::output::Subpixel::VerticalRgb => "vrgb",
        smithay::output::Subpixel::VerticalBgr => "vbgr",
    }
}

pub fn describe_outputs(layout: &Layout<Mapped>, global_space: &Space<Window>) -> Vec<Output> {
    describe_outputs_with_power(layout, global_space, &std::collections::HashMap::new())
}

pub fn describe_outputs_with_power(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
    output_power: &std::collections::HashMap<String, bool>,
) -> Vec<Output> {
    let root_rect = layout
        .monitors()
        .filter_map(|monitor| global_space.output_geometry(monitor.output()))
        .reduce(|a, b| a.merge(b));
    layout
        .monitors()
        .map(|monitor| output_properties(layout, global_space, monitor, output_power, root_rect))
        .collect()
}

/// One monitor's GET_OUTPUTS entry. GET_TREE output nodes reuse it, as sway
/// serialises both from one function (`sway/sway/ipc-json.c:331-372`).
fn output_properties(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
    monitor: &crate::layout::monitor::Monitor<Mapped>,
    output_power: &std::collections::HashMap<String, bool>,
    root_rect: Option<smithay::utils::Rectangle<i32, smithay::utils::Logical>>,
) -> Output {
    let output = monitor.output();
    let mode = output.current_mode();
    let physical = output.physical_properties();
    let transform = sway_transform(output.current_transform());
    let subpixel_hinting = sway_subpixel_hinting(physical.subpixel);
    let current_mode = mode.map_or(
        OutputMode {
            width: 0,
            height: 0,
            refresh: 0,
        },
        |mode| OutputMode {
            width: mode.size.w,
            height: mode.size.h,
            refresh: mode.refresh,
        },
    );
    let powered = output_power
        .get(monitor.output_name())
        .copied()
        .unwrap_or(true);
    // This serializer receives the active layout, not backend connector
    // state. Therefore active is true and primary is false. Runtime
    // power state supplies sway's identical dpms and power fields.
    // Like sway's default scale filter, integer scales use nearest and
    // fractional scales use linear (`sway/sway/config/output.c:650-665`).
    // Backend adaptive-sync, tearing, HDR, and render-time capability/state do
    // not reach this query, so those fields conservatively report their
    // disabled defaults. Keep GET_OUTPUTS documented as Partial until
    // that state is plumbed in.
    Output {
        active: true,
        adaptive_sync_status: "disabled".into(),
        allow_tearing: false,
        border: NodeBorder::None,
        current_border_width: 0,
        current_mode,
        current_workspace: Some(
            monitor
                .active_workspace_ref()
                .sway_name()
                .unwrap_or_else(|| (monitor.active_workspace_idx() + 1).to_string()),
        ),
        deco_rect: Rect::default(),
        dpms: powered,
        features: OutputFeatures {
            adaptive_sync: false,
            hdr: false,
        },
        floating: None,
        floating_nodes: vec![],
        focus: output_focus(monitor),
        focused: layout
            .active_monitor_ref()
            .is_some_and(|active| active.output() == output),
        fullscreen_mode: 0,
        geometry: Rect::default(),
        hdr: false,
        id: output_id(monitor.output_name()),
        layout: NodeLayout::Output,
        make: physical.make.clone(),
        marks: vec![],
        max_render_time: 0,
        model: physical.model.clone(),
        modes: mode
            .into_iter()
            .map(|mode| OutputMode {
                width: mode.size.w,
                height: mode.size.h,
                refresh: mode.refresh,
            })
            .collect(),
        name: monitor.output_name().clone(),
        nodes: vec![],
        non_desktop: false,
        orientation: "none".into(),
        percent: root_rect.and_then(|root| {
            let root_area = i64::from(root.size.w) * i64::from(root.size.h);
            let rect = output_rect(global_space, output);
            let output_area = i64::from(rect.width) * i64::from(rect.height);
            (root_area != 0).then(|| output_area as f64 / root_area as f64)
        }),
        power: powered,
        primary: false,
        rect: output_rect(global_space, output),
        scale: output.current_scale().fractional_scale(),
        scale_filter: if output.current_scale().fractional_scale().fract() == 0. {
            "nearest"
        } else {
            "linear"
        }
        .into(),
        scratchpad_state: None,
        serial: physical.serial_number.clone(),
        sticky: false,
        subpixel_hinting: subpixel_hinting.into(),
        transform: transform.into(),
        node_type: NodeType::Output,
        urgent: false,
        window: None,
        window_rect: Rect::default(),
    }
}

/// The output's workspaces in focus order, most recent first.
///
/// Sway builds an output's `focus` from the seat's focus-inactive children
/// (ipc_json_describe_node, sway/sway/ipc-json.c:827-835), so GET_OUTPUTS and the
/// GET_TREE output node list every workspace on the output, not just the
/// active one. Only workspaces the replies describe are included: an empty,
/// non-persistent, inactive workspace is already gone in sway.
fn output_focus(monitor: &crate::layout::monitor::Monitor<Mapped>) -> Vec<i64> {
    let active = monitor.active_workspace_ref().id();
    let described = |id: WorkspaceId| {
        monitor.sway_workspaces().any(|(_, workspace)| {
            workspace.id() == id && (workspace.must_be_kept() || id == active)
        })
    };
    monitor
        .workspace_focus_history()
        .filter(|id| described(*id))
        .map(|id| workspace_id(id.get()))
        .collect()
}

pub(super) fn describe_output_node(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
    monitor: &crate::layout::monitor::Monitor<Mapped>,
    output_power: &std::collections::HashMap<String, bool>,
    root_rect: Rect,
    marks: &WindowMarks,
    container_marks: &ContainerMarks,
) -> Node {
    let rect = output_rect(global_space, monitor.output());
    // A workspace covers the usable area, not the whole output: sway subtracts
    // every layer-shell exclusive zone, so a bar's strip belongs to the output
    // rect and not to the workspace below it. See the GET_TREE example in
    // sway/sway-ipc.7.scd, where the output is y=0 h=1080 while its workspace
    // is y=23 h=1057 under a 23px bar. Scripts size floating windows from this
    // rect, so reporting the full output puts them under the bar.
    let workspace_rect = workspace_rect(
        global_space,
        monitor.output(),
        monitor.active_workspace_ref(),
    );
    let workspaces = monitor
        .sway_workspaces()
        .filter(|(_, workspace)| {
            workspace.must_be_kept() || monitor.active_workspace_ref().id() == workspace.id()
        })
        .map(|(index, workspace)| {
            describe_workspace_node(WorkspaceNodeContext {
                compositor_layout: layout,
                workspace,
                output: monitor.output_name(),
                index,
                rect: workspace_rect,
                output_origin: rect,
                marks,
                container_marks,
            })
        })
        .collect::<Vec<_>>();
    let focus = output_focus(monitor);
    // The node computes its own percent below, so the GET_OUTPUTS root is unused.
    let output = output_properties(layout, global_space, monitor, output_power, None);
    let mut node = common_node(CommonNodeContext {
        id: output.id,
        node_type: NodeType::Output,
        layout: NodeLayout::Output,
        orientation: "none",
        name: Some(&output.name),
        rect,
        nodes: workspaces,
        floating_nodes: vec![],
        focus,
        focused: false,
        properties: NodeProperties::Output(OutputProperties {
            active: output.active,
            adaptive_sync_status: output.adaptive_sync_status,
            allow_tearing: output.allow_tearing,
            current_mode: output.current_mode,
            current_workspace: output.current_workspace,
            dpms: output.dpms,
            features: output.features,
            hdr: output.hdr,
            make: output.make,
            max_render_time: output.max_render_time,
            model: output.model,
            modes: output.modes,
            non_desktop: output.non_desktop,
            power: output.power,
            primary: output.primary,
            scale: output.scale,
            scale_filter: output.scale_filter,
            serial: output.serial,
            transform: output.transform,
        }),
    });
    let root_area = i64::from(root_rect.width) * i64::from(root_rect.height);
    let output_area = i64::from(rect.width) * i64::from(rect.height);
    node.percent = (root_area != 0).then(|| output_area as f64 / root_area as f64);
    node
}
