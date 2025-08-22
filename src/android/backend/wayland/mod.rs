pub mod bind;
mod compositor;
mod event_centralizer;
mod event_handler;
mod input;
mod keymap;
mod winit_backend;

pub use compositor::{Compositor, State};
pub use event_centralizer::{centralize, CentralizedEvent};
pub use event_handler::handle;
pub use winit_backend::{bind, WinitGraphicsBackend};

use smithay::{
    backend::renderer::gles::GlesRenderer,
    utils::{Clock, Monotonic},
};

pub struct WaylandBackend {
    pub compositor: Compositor,
    pub graphic_renderer: Option<WinitGraphicsBackend<GlesRenderer>>,
    pub clock: Clock<Monotonic>,
    pub key_counter: u32,
    pub scale_factor: f64,
}
