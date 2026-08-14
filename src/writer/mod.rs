//! MDF 4.1-compliant writer module for mf4-rs
//!
//! This module provides a safe, extensible API for writing MDF blocks to disk,
//! guaranteeing little-endian encoding, 8-byte alignment, and zero-padding.

pub mod mdf_writer;
pub use mdf_writer::MdfWriter;
pub use mdf_writer::data::ColumnData;
pub use mdf_writer::InMemorySink;
