//! Frame and configuration lifecycle: refresh, animations, config reloads.

use super::*;

pub(super) fn apply(op: Op, layout: &mut Layout<TestWindow>) -> Applied {
    match op {
        Op::Refresh { is_active } => {
            layout.refresh(is_active);
        }
        Op::AdvanceAnimations { msec_delta } => {
            let mut now = layout.clock.now_unadjusted();
            if msec_delta >= 0 {
                now = now.saturating_add(Duration::from_millis(msec_delta as u64));
            } else {
                now = now.saturating_sub(Duration::from_millis(-msec_delta as u64));
            }
            layout.clock.set_unadjusted(now);
            layout.advance_animations();
        }
        Op::CompleteAnimations => {
            layout.clock.set_complete_instantly(true);
            layout.advance_animations();
            layout.clock.set_complete_instantly(false);
        }
        Op::UpdateConfig { layout_config } => {
            let options = Options {
                layout: swayward_config::Layout::from_part(&layout_config),
                ..Default::default()
            };

            layout.update_options(options);
        }
        Op::SetDefaultOrientation(default_orientation) => {
            let mut options = Options::clone(&layout.options);
            options.layout.default_orientation = default_orientation;
            layout.update_options(options);
        }
        other => return Applied::NotMine(Box::new(other)),
    }
    Applied::Done
}
