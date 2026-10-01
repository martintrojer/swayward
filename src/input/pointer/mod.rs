use super::*;

mod axis;
mod button;
mod buttons;
mod gesture;
mod motion;
mod scroll_binds;

use buttons::{classify_press, floating_drag_policy, PressIntent};
use scroll_binds::synthetic_bind;

impl State {
    pub(super) fn mouse_bind_matches_region(&self, bind: &Bind) -> bool {
        if bind.mouse_regions.is_empty() {
            return true;
        }

        let contents = self
            .swayward
            .contents_under(self.swayward.seat.get_pointer().unwrap().current_location());
        let (click_region, on_workspace) = match contents.window.as_ref().map(|(_, hit)| hit) {
            Some(HitType::Input { .. }) => (MouseRegions::CONTENTS, false),
            Some(HitType::Activate {
                is_tab_indicator: true,
            }) => (MouseRegions::TITLEBAR, false),
            Some(HitType::Activate {
                is_tab_indicator: false,
            }) => (MouseRegions::BORDER, false),
            None if contents.layer.is_none() => (MouseRegions::all(), true),
            None => (MouseRegions::empty(), false),
        };

        mouse_regions_match(bind.mouse_regions, click_region, on_workspace)
    }
}
