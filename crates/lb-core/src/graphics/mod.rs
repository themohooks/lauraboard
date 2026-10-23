//! On-screen graphics configuration and rule evaluation. Rendering lives
//! in `lb-gfx`.
//!
//! * [`model`] — elements, placement, styles, animation
//! * [`shape`] — shape design (gradient, border, slant, shadow…), plate fields
//! * [`config`] — layers, display rules, [`GraphicsConfig`]
//! * [`eval`] — visibility evaluation for the current moment
//! * [`template`] — `{placeholder}` text templates

mod config;
mod eval;
mod model;
mod shape;
mod template;

pub use config::*;
pub use eval::*;
pub use model::*;
pub use shape::*;
pub use template::*;

#[cfg(test)]
mod tests;
