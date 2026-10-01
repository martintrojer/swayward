use super::*;
use crate::utils::transaction::Transaction;
use crate::window::ResolvedWindowRules;

#[derive(Debug)]
pub(super) struct TestWindowInner {
    pub(super) id: usize,
    pub(super) size: Cell<Size<i32, Logical>>,
    pub(super) requested_size: Cell<Option<Size<i32, Logical>>>,
    pub(super) requested_mode: Cell<SizingMode>,
    pub(super) configure_count: Cell<usize>,
    pub(super) received_transaction: Cell<bool>,
    pub(super) interactive_resize: Cell<Option<InteractiveResizeData>>,
    pub(super) has_xdg_decoration: Cell<bool>,
    pub(super) server_side_decoration_requested: Cell<Option<bool>>,
    pub(super) bounds: Cell<Option<Size<i32, Logical>>>,
    pub(super) rules: ResolvedWindowRules,
}

#[derive(Debug, Clone)]
pub(super) struct TestWindow(pub(super) Rc<TestWindowInner>);

impl TestWindow {
    pub(super) fn new(id: usize) -> Self {
        Self::with_rules(id, ResolvedWindowRules::default())
    }

    pub(super) fn with_rules(id: usize, rules: ResolvedWindowRules) -> Self {
        Self(Rc::new(TestWindowInner {
            id,
            size: Cell::new(Size::from((100, 200))),
            requested_size: Cell::new(None),
            requested_mode: Cell::new(SizingMode::Normal),
            configure_count: Cell::new(0),
            received_transaction: Cell::new(false),
            interactive_resize: Cell::new(None),
            has_xdg_decoration: Cell::new(false),
            server_side_decoration_requested: Cell::new(None),
            bounds: Cell::new(None),
            rules,
        }))
    }
}

pub(super) fn tree_with_options(
    size: (f64, f64),
    gaps: f64,
    update: impl FnOnce(&mut Options),
) -> TilingTree<TestWindow> {
    let mut options = Options::default();
    options.layout.gaps = gaps;
    options.layout.border.off = false;
    update(&mut options);
    let size = Size::from(size);
    TilingTree::new(
        size,
        Rectangle::from_size(size),
        false,
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(options),
    )
}

pub(super) fn tree(size: (f64, f64), gaps: f64) -> TilingTree<TestWindow> {
    tree_with_options(size, gaps, |_| {})
}

pub(super) fn tile(id: usize, size: Size<f64, Logical>) -> Tile<TestWindow> {
    Tile::new(
        TestWindow::new(id),
        size,
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    )
}

impl LayoutElement for TestWindow {
    type Id = usize;

    fn id(&self) -> &Self::Id {
        &self.0.id
    }
    fn title(&self) -> String {
        format!("window {}", self.0.id)
    }

    fn size(&self) -> Size<i32, Logical> {
        self.0.size.get()
    }
    fn buf_loc(&self) -> Point<i32, Logical> {
        (0, 0).into()
    }
    fn is_in_input_region(&self, point: Point<f64, Logical>) -> bool {
        Rectangle::from_size(self.size()).to_f64().contains(point)
    }
    fn request_size(
        &mut self,
        size: Size<i32, Logical>,
        mode: SizingMode,
        _: bool,
        transaction: Option<Transaction>,
    ) {
        self.0.requested_size.set(Some(size));
        self.0.received_transaction.set(transaction.is_some());
        self.0.requested_mode.set(mode);
    }
    fn min_size(&self) -> Size<i32, Logical> {
        Size::from((0, 0))
    }
    fn max_size(&self) -> Size<i32, Logical> {
        Size::from((0, 0))
    }
    fn is_wl_surface(&self, _: &WlSurface) -> bool {
        false
    }
    fn set_preferred_scale_transform(&self, _: output::Scale, _: Transform) {}
    fn has_ssd(&self) -> bool {
        false
    }
    fn output_enter(&self, _: &Output) {}
    fn output_leave(&self, _: &Output) {}
    fn set_offscreen_data(&self, _: Option<OffscreenData>) {}
    fn has_xdg_decoration(&self) -> bool {
        self.0.has_xdg_decoration.get()
    }
    fn request_server_decoration(&mut self, server_side: bool) {
        self.0
            .server_side_decoration_requested
            .set(Some(server_side));
    }
    fn set_activated(&mut self, _: bool) {}
    fn set_bounds(&self, bounds: Size<i32, Logical>) {
        self.0.bounds.set(Some(bounds));
    }
    fn is_ignoring_opacity_window_rule(&self) -> bool {
        false
    }
    fn configure_intent(&self) -> ConfigureIntent {
        ConfigureIntent::CanSend
    }
    fn send_pending_configure(&mut self) {
        self.0.configure_count.set(self.0.configure_count.get() + 1);
    }
    fn set_active_in_column(&mut self, _: bool) {}
    fn set_floating(&mut self, _: bool) {}
    fn sizing_mode(&self) -> SizingMode {
        SizingMode::Normal
    }
    fn pending_sizing_mode(&self) -> SizingMode {
        self.0.requested_mode.get()
    }
    fn requested_size(&self) -> Option<Size<i32, Logical>> {
        self.0.requested_size.get()
    }
    fn is_child_of(&self, _: &Self) -> bool {
        false
    }
    fn refresh(&self) {}
    fn rules(&self) -> &ResolvedWindowRules {
        &self.0.rules
    }
    fn take_animation_snapshot(&mut self) -> Option<LayoutElementRenderSnapshot> {
        None
    }
    fn set_interactive_resize(&mut self, data: Option<InteractiveResizeData>) {
        self.0.interactive_resize.set(data);
    }
    fn cancel_interactive_resize(&mut self) {}
    fn on_commit(&mut self, _: Serial) {}
    fn interactive_resize_data(&self) -> Option<InteractiveResizeData> {
        None
    }
    fn is_urgent(&self) -> bool {
        false
    }
}

/// The `deco_rect` GET_TREE reports for `window`: the IPC leaf's, from the
/// same snapshot the wire serialises.
pub(super) fn ipc_deco_rect(
    t: &TilingTree<TestWindow>,
    window: usize,
) -> Option<Rectangle<f64, Logical>> {
    fn find(node: &IpcNode<usize>, window: usize) -> Option<Option<Rectangle<f64, Logical>>> {
        match node {
            IpcNode::Leaf {
                window: id,
                deco_rect,
                ..
            } => (*id == window).then_some(*deco_rect),
            IpcNode::Split { children, .. } => {
                children.iter().find_map(|child| find(child, window))
            }
        }
    }
    find(&t.ipc_tree(), window).unwrap_or_else(|| panic!("window {window} not in the IPC tree"))
}
