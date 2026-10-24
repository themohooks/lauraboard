//! On-screen graphics configuration and rule evaluation. Rendering lives
//! in `lb-gfx`.
//!
//! * [`model`] — elements, placement, styles, animation
//! * [`shape`] — shape design (gradient, border, slant, shadow…), plate fields
//! * [`config`] — layers, display rules, [`GraphicsConfig`]
//! * [`eval`] — visibility evaluation for the current moment
//! * [`template`] — `{placeholder}` text templates
//! * [`program`] — timing of the programme on air (countdown, progress bar)
//! * [`scene`] — a scene's own graphics merged in while it is on air

mod config;
mod eval;
mod model;
mod program;
mod scene;
mod shape;
mod template;

pub use config::*;
pub use eval::*;
pub use model::*;
pub use program::*;
pub use scene::*;
pub use shape::*;
pub use template::*;

#[cfg(test)]
mod tests;
