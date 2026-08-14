//! Minimal utilities for reading and writing ASAM MDF 4 files.
//!
//! The crate exposes a high level API under [`api`] to inspect existing
//! recordings as well as a [`writer::MdfWriter`] to generate new files.  Only a
//! fraction of the MDF 4 specification is implemented.

pub mod blocks;
pub mod error;
pub mod writer;
/// File-cutting utilities. Path-based entry points are native only; the
/// in-memory `*_bytes` variants are available on all targets (including wasm).
pub mod cut;
/// File-merging utilities. The path-based entry point is native only; the
/// in-memory `merge_files_bytes` variant is available on all targets.
pub mod merge;
pub mod index;
pub mod signal;
pub mod block_layout;

pub mod parsing {
    pub mod decoder;
    pub mod mdf_file;
    pub mod raw_channel_group;
    pub mod raw_data_group;
    pub mod raw_channel;
    pub mod source_info;
    pub(crate) mod reader_walk;
}

pub mod api {
    pub mod mdf;
    pub mod channel_group;
    pub mod channel;
}

// Python bindings module
#[cfg(feature = "pyo3")]
pub mod python;

// Shared, target-neutral fragment-backed byte-range readers. Used by both
// the wasm-bindgen (`wasm`) and WASI Preview 2 component (`wasip2`) bindings.
#[cfg(any(feature = "wasm", feature = "wasip2"))]
pub(crate) mod fragments;

// WebAssembly (wasm-bindgen) bindings module
#[cfg(feature = "wasm")]
pub mod wasm;

// WASI Preview 2 (WebAssembly Component) bindings module
#[cfg(feature = "wasip2")]
pub mod wasi;

// Re-export the Python module when building as an extension
#[cfg(feature = "pyo3")]
use pyo3::prelude::*;

/// Python bindings for the ``mf4-rs`` crate: a minimal reader/writer for
/// ASAM MDF 4 measurement files.
///
/// Everything is addressed by **name** — channel-group and channel names —
/// never by numeric index.
///
/// Quick tour
/// ----------
///
/// Read — ``read`` returns a ``pandas.Series`` (values indexed by the master
/// time axis); ``values`` returns a plain numpy array::
///
///     import mf4_rs
///     mdf = mf4_rs.Mdf("recording.mf4")
///     for g in mdf.groups:
///         print(g.name, g.record_count, g.channel_names)
///     speed = mdf["Speed"]                 # pandas Series, datetime index
///     rpm   = mdf.read("RPM", group="Engine")
///     raw   = mdf.values("Speed")          # numpy.ndarray[float64], no index
///
/// Write::
///
///     w = mf4_rs.MdfWriter("out.mf4")
///     w.init_mdf_file()
///     cg = w.add_channel_group("Engine")
///     t  = w.add_time_channel(cg, "Time")
///     y  = w.add_float_channel(cg, "Speed")
///     w.start_data_block(cg)
///     w.write_record(cg, [
///         mf4_rs.create_float_value(0.0),
///         mf4_rs.create_float_value(123.4),
///     ])
///     w.finish_data_block(cg)
///     w.finalize()
///
/// Index (HTTP-friendly random access) — the index remembers its source and
/// reads lazily (range requests happen on ``read``/``values``, not at build)::
///
///     idx = mf4_rs.MdfIndex.from_url("https://host/recording.mf4")  # only metadata fetched
///     speed = idx.read("Speed")            # pandas Series; range request happens now
///     idx.save("recording.idx.json")
///     idx = mf4_rs.MdfIndex.load("recording.idx.json")
///     idx.source = "recording.mf4"         # re-attach a source after load
///     raw = idx.values("Speed")            # numpy, lazy
///
/// Other utilities: :func:`merge_files`, :func:`cut_mdf_by_time`,
/// :func:`cut_mdf_by_utc`, and the :class:`FileLayout` block-layout
/// inspector.
///
/// All errors raised by this module are subclasses of :class:`MdfException`.
#[cfg(feature = "pyo3")]
#[pymodule]
fn _mf4_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    python::init_mf4_rs_module(m)
}
