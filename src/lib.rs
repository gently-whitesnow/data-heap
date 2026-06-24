#![forbid(unsafe_code)]

//! data-heap: text ingestion service core.
//!
//! Clean architecture / ports & adapters. The [`domain`] layer holds entities and
//! trait-ports and knows nothing about infrastructure; [`adapters`] implement those
//! ports (SQLite [`Storage`](domain::ports::Storage) is the first one), and
//! [`daemon`] wires everything together at runtime.

pub mod adapters;
pub mod config;
pub mod daemon;
pub mod domain;

pub use domain::error::{Error, Result};
