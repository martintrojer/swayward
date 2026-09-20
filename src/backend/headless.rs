//! Headless backend for tests.
//!
//! This can eventually grow into a more complete backend if needed, but for now it's missing some
//! crucial parts like dmabufs.

use std::mem;
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::egl::native::EGLSurfacelessDisplay;
use smithay::backend::egl::{EGLContext, EGLDisplay};
use smithay::backend::renderer::element::RenderElementStates;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::output::{Mode, Output, PhysicalProperties, Subpixel};
use smithay::reexports::wayland_protocols::wp::presentation_time::server::wp_presentation_feedback;
use smithay::utils::Size;
use smithay::wayland::presentation::Refresh;
use swayward_config::OutputName;

use super::{IpcOutputMap, OutputId, RenderResult};
use crate::render_helpers::{resources, shaders};
use crate::swayward::{RedrawState, Swayward};
use crate::utils::{get_monotonic_time, logical_output};

pub struct Headless {
    renderer: Option<GlesRenderer>,
    ipc_outputs: Arc<Mutex<IpcOutputMap>>,
    last_output_number: u8,
}

impl Headless {
    pub fn new() -> Self {
        Self {
            renderer: None,
            ipc_outputs: Default::default(),
            last_output_number: 0,
        }
    }

    pub fn init(&mut self, _niri: &mut Swayward) {}

    pub fn add_renderer(&mut self) -> anyhow::Result<()> {
        if self.renderer.is_some() {
            error!("add_renderer: renderer must not already exist");
            return Ok(());
        }

        let mut renderer = unsafe {
            let display =
                EGLDisplay::new(EGLSurfacelessDisplay).context("error creating EGL display")?;
            let context = EGLContext::new(&display).context("error creating EGL context")?;
            GlesRenderer::new(context).context("error creating renderer")?
        };

        resources::init(&mut renderer);
        shaders::init(&mut renderer);

        self.renderer = Some(renderer);
        Ok(())
    }

    pub fn add_output(&mut self, swayward: &mut Swayward, n: u8, size: (u16, u16)) {
        self.add_output_at(swayward, n, size, None);
    }

    pub fn create_output(&mut self, swayward: &mut Swayward) -> Result<(), &'static str> {
        let next = self
            .last_output_number
            .checked_add(1)
            .ok_or("Could not create output")?;
        self.add_output(swayward, next, (1920, 1080));
        Ok(())
    }

    pub fn add_output_at(
        &mut self,
        swayward: &mut Swayward,
        n: u8,
        size: (u16, u16),
        position: Option<(i32, i32)>,
    ) {
        self.add_named_output_at(swayward, format!("headless-{n}"), size, position);
    }

    pub fn add_named_output_at(
        &mut self,
        swayward: &mut Swayward,
        connector: String,
        size: (u16, u16),
        position: Option<(i32, i32)>,
    ) {
        if let Some(number) = connector
            .strip_prefix("headless-")
            .and_then(|number| number.parse::<u8>().ok())
        {
            self.last_output_number = self.last_output_number.max(number);
        }
        let make = "swayward".to_string();
        let model = "headless".to_string();
        let serial = connector.clone();

        let output = Output::new(
            connector.clone(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: make.clone(),
                model: model.clone(),
                serial_number: serial.clone(),
            },
        );

        let mode = Mode {
            size: Size::from((i32::from(size.0), i32::from(size.1))),
            refresh: 60_000,
        };
        output.change_current_state(Some(mode), None, None, None);
        output.set_preferred(mode);

        output.user_data().insert_if_missing(|| OutputName {
            connector: connector.clone(),
            make: Some(make),
            model: Some(model),
            serial: Some(serial),
        });

        let physical_properties = output.physical_properties();
        self.ipc_outputs.lock().unwrap().insert(
            OutputId::next(),
            swayward_ipc::legacy::Output {
                name: output.name(),
                make: physical_properties.make,
                model: physical_properties.model,
                serial: None,
                physical_size: None,
                modes: vec![swayward_ipc::Mode {
                    width: size.0,
                    height: size.1,
                    refresh_rate: 60_000,
                    is_preferred: true,
                }],
                current_mode: Some(0),
                is_custom_mode: true,
                vrr_supported: false,
                vrr_enabled: false,
                logical: Some(logical_output(&output)),
                max_bpc: None,
            },
        );

        if let Some((x, y)) = position {
            swayward
                .config
                .borrow_mut()
                .outputs
                .0
                .push(swayward_config::Output {
                    name: connector.clone(),
                    position: Some(swayward_config::Position { x, y }),
                    ..Default::default()
                });
        }
        swayward.add_output(output, None, false);
    }

    pub fn retain_ipc_outputs(&mut self, names: &[String]) {
        self.ipc_outputs
            .lock()
            .unwrap()
            .retain(|_, output| names.contains(&output.name));
    }

    pub fn seat_name(&self) -> String {
        "headless".to_owned()
    }

    pub fn with_primary_renderer<T>(
        &mut self,
        f: impl FnOnce(&mut GlesRenderer) -> T,
    ) -> Option<T> {
        self.renderer.as_mut().map(f)
    }

    pub fn render(&mut self, swayward: &mut Swayward, output: &Output) -> RenderResult {
        let states = RenderElementStates::default();
        let mut presentation_feedbacks = swayward.take_presentation_feedbacks(output, &states);
        presentation_feedbacks.presented::<_, smithay::utils::Monotonic>(
            get_monotonic_time(),
            Refresh::Unknown,
            0,
            wp_presentation_feedback::Kind::empty(),
        );

        let output_state = swayward.output_state.get_mut(output).unwrap();
        match mem::replace(&mut output_state.redraw_state, RedrawState::Idle) {
            RedrawState::Idle => unreachable!(),
            RedrawState::Queued => (),
            RedrawState::WaitingForVBlank { .. } => unreachable!(),
            RedrawState::WaitingForEstimatedVBlank(_) => unreachable!(),
            RedrawState::WaitingForEstimatedVBlankAndQueued(_) => unreachable!(),
        }

        output_state.frame_callback_sequence = output_state.frame_callback_sequence.wrapping_add(1);

        // FIXME: request redraw on unfinished animations remain

        RenderResult::Submitted
    }

    pub fn import_dmabuf(&mut self, _dmabuf: &Dmabuf) -> bool {
        // Only the tty and winit backends create the linux-dmabuf global, so a
        // client can never reach this on headless. Refuse the import instead of
        // panicking: the caller answers the client with a failure notifier.
        false
    }

    pub fn ipc_outputs(&self) -> Arc<Mutex<IpcOutputMap>> {
        self.ipc_outputs.clone()
    }
}

impl Default for Headless {
    fn default() -> Self {
        Self::new()
    }
}
