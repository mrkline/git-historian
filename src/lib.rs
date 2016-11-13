//! This crate analyzes a Git repository (by parsing `git log --name-status`),
//! then builds a tree of the history for a provided list of files.
//!
//! At each node (corresponding to a delta in the file's history),
//! a user-provided callback is issued to gather desired information.
//!
//! See `main.rs` for a quick demo.

extern crate time;

pub mod parsing;
pub mod history;

pub use self::types::*;

mod types;
