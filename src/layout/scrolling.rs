//! Compatibility types retained from niri's removed scrolling layout engine.
//!
//! The engine itself was replaced by [`super::tiling_tree::TilingTree`]. These
//! types remain because inherited actions and floating/workspace transfer APIs
//! still express horizontal direction and requested tile width in niri's column
//! vocabulary.

/// Width requested for a tiled window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnWidth {
    /// Proportion of the current view width.
    Proportion(f64),
    /// Fixed width in logical pixels.
    Fixed(f64),
}

/// Horizontal direction for inherited column-oriented actions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDirection {
    Left,
    Right,
}
