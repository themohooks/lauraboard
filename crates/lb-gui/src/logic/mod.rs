//! UI logic without toolkit dependencies (unit-tested).
//!
//! * [`undo`] — undo / redo history of document drafts
//! * [`filter`] — media search, filters, sorting
//! * [`timeline`] — plan → day segments, as-run lane, time parsing
//! * [`rule_check`] — rule conflicts, broken references, dependencies

pub mod filter;
pub mod rule_check;
pub mod timeline;
pub mod undo;
