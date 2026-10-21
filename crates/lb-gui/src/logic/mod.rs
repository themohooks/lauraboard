//! UI logic without toolkit dependencies (unit-tested).
//!
//! * [`undo`] — undo / redo history of document drafts
//! * [`filter`] — media search, filters, sorting
//! * [`timeline`] — plan → day segments, as-run lane, time parsing

pub mod filter;
pub mod timeline;
pub mod undo;
