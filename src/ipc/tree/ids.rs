use super::*;

pub(super) fn ipc_layout(layout: TreeLayout) -> NodeLayout {
    match layout {
        TreeLayout::SplitH => NodeLayout::SplitH,
        TreeLayout::SplitV => NodeLayout::SplitV,
        TreeLayout::Tabbed => NodeLayout::Tabbed,
        TreeLayout::Stacked => NodeLayout::Stacked,
    }
}
pub(super) fn orientation(layout: TreeLayout) -> &'static str {
    match layout {
        TreeLayout::SplitH => "horizontal",
        TreeLayout::SplitV => "vertical",
        TreeLayout::Tabbed | TreeLayout::Stacked => "none",
    }
}
pub(super) fn output_id(name: &str) -> i64 {
    OUTPUT_ID_BASE + stable_hash(name)
}
pub(crate) fn workspace_id(id: u64) -> i64 {
    WORKSPACE_ID_BASE + i64::try_from(id % ID_NAMESPACE_SIZE as u64).unwrap_or_default()
}
pub(crate) fn container_id(id: NodeId) -> i64 {
    CONTAINER_ID_BASE + i64::try_from(id.0 % ID_NAMESPACE_SIZE as u64).unwrap_or_default()
}
pub(crate) fn window_id(id: MappedId) -> i64 {
    window_id_from_raw(id.get())
}
pub(crate) fn window_id_from_raw(id: u64) -> i64 {
    WINDOW_ID_BASE + i64::try_from(id % ID_NAMESPACE_SIZE as u64).unwrap_or_default()
}
pub(super) fn stable_hash(value: &str) -> i64 {
    value.bytes().fold(0i64, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(i64::from(byte))
    }) % ID_NAMESPACE_SIZE
}
