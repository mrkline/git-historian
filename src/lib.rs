//! A library for parsing Git output into a history graph.

extern crate time;

pub mod parsing;
pub mod history;

pub use self::types::*;

mod types;
