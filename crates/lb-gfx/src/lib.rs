//! Broadcast graphics compositor: renders the active graphic elements into
//! one RGBA frame (premultiplied alpha) that the encoder overlays on video.
//!
//! * [`compositor`] — frame assembly, caching, element dispatch
//! * [`elements`] — renderers per element kind (image, text, ticker, …)
//! * [`text`] — glyph layout and vector text rasterisation (fill/outline/shadow)
//! * [`shape`] — designed shapes: gradient, corners, slant, shadow, accent
//! * [`plate`] — plates: a shape with lines of text
//! * [`fonts`] — font lookup (system families or font files)
//! * [`images`] — image loading and scaled caching
//! * [`layout`] — anchors and placement maths
//! * [`anim`] — in/out animation curves
//! * [`color`] — colour parsing
//! * [`backdrops`] — ready-made backgrounds and plate textures

pub mod anim;
pub mod backdrops;
pub mod color;
pub mod compositor;
pub mod elements;
pub mod fonts;
pub mod images;
pub mod layout;
pub mod plate;
pub mod shape;
pub mod text;
pub mod web;

pub use compositor::{Compositor, FrameContext};
