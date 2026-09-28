use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn new(
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        gaps_to_edge: bool,
        scale: f64,
        clock: Clock,
        options: Rc<Options>,
    ) -> Self {
        let root = NodeId(NODE_ID_COUNTER.next());
        let root_layout = match options.layout.default_orientation {
            swayward_config::DefaultOrientation::Horizontal => Layout::SplitH,
            swayward_config::DefaultOrientation::Vertical => Layout::SplitV,
            swayward_config::DefaultOrientation::Auto if view_size.h > view_size.w => {
                Layout::SplitV
            }
            swayward_config::DefaultOrientation::Auto => Layout::SplitH,
        };
        let nodes = HashMap::from([(
            root,
            Node {
                parent: None,
                value: TreeNode::Split {
                    layout: root_layout,
                    children: Vec::new(),
                    percents: Vec::new(),
                },
            },
        )]);
        Self {
            nodes,
            root,
            focus: None,
            has_had_tile: false,
            empty_representation_layout: None,
            focus_history: Vec::new(),
            previous_split_layouts: HashMap::new(),
            title_formats: HashMap::new(),
            pending_modes: HashMap::new(),
            mapped_under_fullscreen: HashSet::new(),
            fullscreen_layout_wrappers: HashSet::new(),
            pre_layout_ipc_rects: HashMap::new(),
            interactive_resize: None,
            tab_indicators: HashMap::new(),
            titlebars: Default::default(),
            tab_active: HashMap::new(),
            closing_windows: Vec::new(),
            view_size,
            parent_area,
            gaps_to_edge,
            resident_root: false,
            scale,
            titlebar_height: crate::layout::titlebar::height(scale, &options.layout.titlebar),
            clock,
            gaps: options.layout.gaps,
            options,
            preserved_auto_layout: None,
        }
    }

    pub fn from_detached_subtree(
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        scale: f64,
        clock: Clock,
        options: Rc<Options>,
        subtree: DetachedSubtree<W>,
    ) -> (Self, NodeId, Vec<(NodeId, NodeId)>) {
        let mut tree = Self::new(view_size, parent_area, false, scale, clock, options);
        tree.resident_root = true;
        let focus_history = subtree.focus_history;
        let root_focused = subtree.root_focused;
        let mut remapped = Vec::new();
        let id = tree.insert_detached_node(subtree.node, None, &mut remapped);
        tree.insert_child(tree.root, id, None);
        tree.restore_transferred_focus(focus_history);
        if root_focused {
            tree.set_focus(id);
        }
        tree.has_had_tile = true;
        tree.request_window_sizes();
        (tree, id, remapped)
    }

    pub fn resident_root(&self) -> Option<NodeId> {
        if !self.resident_root {
            return None;
        }
        let TreeNode::Split { children, .. } = &self.nodes[&self.root].value else {
            unreachable!()
        };
        (children.len() == 1).then(|| children[0])
    }

    pub fn detach_resident_root(mut self, id: NodeId) -> Option<DetachedSubtree<W>> {
        (self.resident_root()? == id)
            .then(|| self.detach_subtree(id).map(|(subtree, _)| subtree))
            .flatten()
    }

    pub fn is_empty(&self) -> bool {
        self.focus.is_none()
    }

    pub fn has_had_tile(&self) -> bool {
        self.has_had_tile
    }

    pub fn representation_layout(&self) -> Layout {
        let TreeNode::Split { layout, .. } = self.nodes[&self.root].value else {
            unreachable!()
        };
        if self.is_empty() {
            self.empty_representation_layout.unwrap_or(layout)
        } else {
            layout
        }
    }

    pub fn reset_empty_layout(&mut self) {
        assert!(self.is_empty());
        let layout = match self.preserved_auto_layout {
            Some(layout) => layout,
            None => match self.options.layout.default_orientation {
                swayward_config::DefaultOrientation::Horizontal => Layout::SplitH,
                swayward_config::DefaultOrientation::Vertical => Layout::SplitV,
                swayward_config::DefaultOrientation::Auto
                    if self.view_size.h > self.view_size.w =>
                {
                    Layout::SplitV
                }
                swayward_config::DefaultOrientation::Auto => Layout::SplitH,
            },
        };
        self.set_layout(self.root, layout);
        self.empty_representation_layout = Some(layout);
    }

    pub fn preserve_empty_auto_layout(&mut self) {
        let TreeNode::Split { layout, .. } = self.nodes[&self.root].value else {
            unreachable!();
        };
        self.preserved_auto_layout = Some(layout);
    }

    pub fn track_empty_auto_layout(&mut self) {
        let Some(preserved) = self.preserved_auto_layout.take() else {
            return;
        };
        if self.is_empty()
            && self.options.layout.default_orientation == swayward_config::DefaultOrientation::Auto
            && matches!(
                self.nodes[&self.root].value,
                TreeNode::Split { layout, .. } if layout == preserved
            )
        {
            self.reset_empty_layout();
        }
    }

    pub(super) fn update_empty_auto_layout(&mut self, view_size: Size<f64, Logical>) {
        let old_auto_layout = if self.view_size.h > self.view_size.w {
            Layout::SplitV
        } else {
            Layout::SplitH
        };
        let new_auto_layout = if view_size.h > view_size.w {
            Layout::SplitV
        } else {
            Layout::SplitH
        };
        if self.preserved_auto_layout.is_none()
            && self.is_empty()
            && self.options.layout.default_orientation == swayward_config::DefaultOrientation::Auto
            && matches!(
                self.nodes[&self.root].value,
                TreeNode::Split { layout, .. } if layout == old_auto_layout
            )
        {
            self.set_layout(self.root, new_auto_layout);
        }
    }

    pub fn update_config(
        &mut self,
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        gaps_to_edge: bool,
        scale: f64,
        options: Rc<Options>,
    ) {
        for tile in self.tiles_mut() {
            tile.update_config(view_size, scale, options.clone());
        }
        for indicator in self.tab_indicators.values_mut() {
            indicator.update_config(options.layout.tab_indicator);
        }
        self.update_empty_auto_layout(view_size);
        self.view_size = view_size;
        self.parent_area = parent_area;
        self.gaps_to_edge = gaps_to_edge;
        self.scale = scale;
        self.titlebar_height = crate::layout::titlebar::height(scale, &options.layout.titlebar);
        self.gaps = options.layout.gaps;
        self.options = options;
        self.request_window_sizes_with(None, false);
    }

    pub fn update_shaders(&mut self) {
        for tile in self.tiles_mut() {
            tile.update_shaders();
        }
        for indicator in self.tab_indicators.values_mut() {
            indicator.update_shaders();
        }
    }

    pub fn advance_animations(&mut self) {
        for tile in self.tiles_mut() {
            tile.advance_animations();
        }
        for indicator in self.tab_indicators.values_mut() {
            indicator.advance_animations();
        }
        self.closing_windows.retain_mut(|closing| {
            closing.advance_animations();
            closing.are_animations_ongoing()
        });
    }

    pub fn are_animations_ongoing(&self) -> bool {
        self.tiles().any(Tile::are_animations_ongoing)
            || self
                .tab_indicators
                .values()
                .any(TabIndicator::are_animations_ongoing)
            || !self.closing_windows.is_empty()
    }

    pub fn are_transitions_ongoing(&self) -> bool {
        self.tiles().any(Tile::are_transitions_ongoing)
            || self
                .tab_indicators
                .values()
                .any(TabIndicator::are_animations_ongoing)
            || !self.closing_windows.is_empty()
    }

    pub fn view_size(&self) -> Size<f64, Logical> {
        self.view_size
    }

    pub fn parent_area(&self) -> Rectangle<f64, Logical> {
        self.parent_area
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn options(&self) -> &Rc<Options> {
        &self.options
    }

    pub fn new_window_toplevel_bounds(&self, rules: &ResolvedWindowRules) -> Size<i32, Logical> {
        let border = self.options.layout.border.merged_with(&rules.border);
        let mut size = self.working_area().size;
        let padding = self.gaps * 2. + if border.off { 0. } else { border.width * 2. };
        size.w = (size.w - padding).max(1.);
        size.h = (size.h - padding).max(1.);
        size.to_i32_floor()
    }

    /// Return a new tiled view's initial size.
    ///
    /// Tree leaves consume the complete allocated width. Applying niri's default column width
    /// before insertion would make the first leaf too narrow until it acknowledges another
    /// configure. The inherited height preset remains supported independently.
    pub fn new_window_size(
        &self,
        height: Option<PresetSize>,
        rules: &ResolvedWindowRules,
    ) -> Size<i32, Logical> {
        let bounds = self.new_window_toplevel_bounds(rules);
        let height = match height {
            Some(PresetSize::Fixed(value)) => value.max(1),
            Some(PresetSize::Proportion(value)) => {
                (f64::from(bounds.h) * value).floor().max(1.) as i32
            }
            None => bounds.h,
        };
        Size::from((bounds.w, height))
    }
}
