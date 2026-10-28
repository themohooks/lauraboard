//! UI logic without toolkit dependencies (unit-tested).
//!
//! * [`undo`] — undo / redo history of document drafts
//! * [`filter`] — media search, filters, sorting
//! * [`timeline`] — plan → day segments, as-run lane, time parsing
//! * [`rule_check`] — rule conflicts, broken references, dependencies
//! * [`placement`] — graphics canvas geometry, snapping, resizing

pub mod filter;
pub mod mask;
pub mod period;
pub mod placement;
pub mod rule_check;
pub mod timeline;
pub mod undo;

pub mod slug;
