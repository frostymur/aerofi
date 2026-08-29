//! Pure business logic: target parsing, directory indexing, fuzzy search,
//! execution. Knows nothing about GPUI or platform FFI.

pub mod config;
pub mod executor;
pub mod history;
pub mod item;
pub mod scanner;
pub mod search;
