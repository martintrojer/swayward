//! Gaps, borders and titlebars.

use super::*;

#[test]
fn floating_normal_border_has_titlebar_above_squared_client() {
    let radius = swayward_config::CornerRadius {
        top_left: 11.,
        top_right: 12.,
        bottom_right: 13.,
        bottom_left: 14.,
    };
    let config = Config::load_default();
    let mut layout = check_ops_with_options(
        Options {
            layout: config.layout,
            ..Default::default()
        },
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams {
                    is_floating: true,
                    rules: Some(ResolvedWindowRules {
                        geometry_corner_radius: Some(radius),
                        clip_to_geometry: Some(true),
                        ..Default::default()
                    }),
                    ..TestWindowParams::new(1)
                },
            },
        ],
    );
    layout.update_render_elements(None);

    let workspace = layout.active_workspace().unwrap();
    let floating = workspace.floating();
    let (tile, ipc) = floating.tiles_with_ipc_layouts().next().unwrap();
    assert!(tile.has_sway_titlebar());
    assert_eq!(tile.geometry_corner_radius(), radius);
    let expected_outer = tile.geometry_corner_radius();
    assert_eq!(
        tile.decoration_corner_radii(),
        (
            Some(expected_outer),
            swayward_config::CornerRadius {
                top_left: 0.,
                top_right: 0.,
                ..expected_outer
            }
        )
    );
    let titlebar = floating.ipc_decoration_rect(tile, &ipc).unwrap();
    let tile_pos: Point<f64, Logical> = ipc.tile_pos_in_workspace_view.unwrap().into();
    assert_eq!(titlebar.loc.y, tile_pos.y);
    assert_eq!(titlebar.loc.x, tile_pos.x);
    assert_eq!(titlebar.size.w, ipc.tile_size.0);
    let (window, hit) = workspace
        .window_under(titlebar.loc + titlebar.size.downscale(2.).to_point())
        .unwrap();
    assert_eq!(window.id(), &1);
    assert_eq!(
        hit,
        HitType::Activate {
            is_tab_indicator: true
        }
    );
}

#[test]
fn outer_gaps_change_tiled_and_floating_workspace_geometry() {
    let mut options = Options::default();
    options.layout.border.off = true;
    options.layout.gaps = 0.;
    options.layout.outer_gaps = swayward_config::OuterGaps {
        left: 30.,
        right: 10.,
        top: 20.,
        bottom: 0.,
    };
    options.layout.outer_gaps_configured = true;
    let mut floating = TestWindowParams::new(2);
    floating.is_floating = true;
    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::AddWindow { params: floating },
        ],
    );

    let workspace = layout.active_workspace().unwrap();
    assert_eq!(
        workspace.working_area(),
        Rectangle::new((30., 20.).into(), (1240., 700.).into())
    );
    let tiled = workspace
        .tiling()
        .tiles_with_ipc_layouts()
        .find(|(tile, _)| tile.window().id() == &1)
        .unwrap()
        .1;
    assert_eq!(tiled.tile_pos_in_workspace_view, Some((30., 20.)));
    let floating = workspace
        .floating()
        .tiles_with_ipc_layouts()
        .find(|(tile, _)| tile.window().id() == &2)
        .unwrap()
        .1;
    assert_eq!(floating.tile_pos_in_workspace_view, Some((600., 270.)));
}

#[test]
fn smart_borders_no_gaps_uses_resolved_workspace_gaps() {
    for ((inner, outer), expected) in [
        ((0., 5.), ResizeEdge::all()),
        ((10., -2.), ResizeEdge::all()),
        ((10., -10.), ResizeEdge::empty()),
    ] {
        let mut options = Options::default();
        options.layout.border.off = false;
        options.layout.gaps = inner;
        options.layout.outer_gaps = swayward_config::OuterGaps::all(outer);
        options.layout.outer_gaps_configured = true;
        options.layout.smart_borders = swayward_config::SmartBorders::NoGaps;
        let layout = check_ops_with_options(
            options,
            [
                Op::AddOutput(1),
                Op::AddWindow {
                    params: TestWindowParams::new(1),
                },
            ],
        );

        let node = layout.active_workspace().unwrap().ipc_tiling_tree();
        let IpcNode::Split { children, .. } = node else {
            panic!("root must be a split")
        };
        let IpcNode::Leaf { border_edges, .. } = children[0] else {
            panic!("window must be a leaf")
        };
        assert_eq!(border_edges, expected, "outer gap {outer}");
    }
}

#[test]
fn negative_outer_gaps_add_to_inner_gaps() {
    let mut options = Options::default();
    options.layout.gaps = 10.;
    options.layout.outer_gaps = swayward_config::OuterGaps::all(-2.);
    options.layout.outer_gaps_configured = true;
    let layout = check_ops_with_options(options, [Op::AddOutput(1)]);

    assert_eq!(
        layout.active_workspace().unwrap().working_area(),
        Rectangle::new((8., 8.).into(), (1264., 704.).into())
    );
}

#[test]
fn outer_gaps_use_sways_proportional_minimum_size_clamp() {
    let area = workspace::apply_outer_gaps(
        Rectangle::from_size(Size::from((150., 100.))),
        swayward_config::OuterGaps {
            left: 100.,
            right: 300.,
            top: 20.,
            bottom: 60.,
        },
        0.,
        true,
    );

    assert_eq!(area, Rectangle::new((12., 10.).into(), (100., 60.).into()));
}
