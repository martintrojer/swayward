use std::cell::RefCell;
use std::rc::Rc;

use smithay::backend::renderer::element::RenderElement;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::utils::{Physical, Size};
use swayward::ui::hotkey_overlay::HotkeyOverlay;
use swayward_config::{Config, ModKey};

use super::{Args, TestCase};

pub struct Hotkeys {
    output: Output,
    overlay: HotkeyOverlay,
}

impl Hotkeys {
    pub fn new(args: Args) -> Self {
        let output = Output::new(
            String::new(),
            PhysicalProperties {
                size: Size::from((args.size.w, args.size.h)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        output.change_current_state(
            Some(Mode {
                size: args.size.to_physical(1),
                refresh: 60000,
            }),
            None,
            None,
            None,
        );
        let mut overlay =
            HotkeyOverlay::new(Rc::new(RefCell::new(Config::load_default())), ModKey::Super);
        overlay.show();
        Self { output, overlay }
    }
}

impl TestCase for Hotkeys {
    fn resize(&mut self, width: i32, height: i32) {
        self.output.change_current_state(
            Some(Mode {
                size: Size::from((width, height)),
                refresh: 60000,
            }),
            None,
            None,
            None,
        );
    }

    fn render(
        &mut self,
        renderer: &mut GlesRenderer,
        _size: Size<i32, Physical>,
    ) -> Vec<Box<dyn RenderElement<GlesRenderer>>> {
        self.overlay
            .render(renderer, &self.output)
            .map(|element| Box::new(element) as Box<dyn RenderElement<GlesRenderer>>)
            .into_iter()
            .collect()
    }
}
