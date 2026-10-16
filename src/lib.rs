//! Sift Mail: a calm, fast, keyboard-first desktop email client built on GPUI Kit.
//!
//! The library owns everything the executable runs: brand strings, the design
//! tokens and theme, mail domain types, and the UI. `main.rs` only wires the
//! process together, so all behaviour here is reachable from tests.

pub mod brand;
pub mod domain;
pub mod store;
pub mod sync;
pub mod theme;
pub mod ui;
