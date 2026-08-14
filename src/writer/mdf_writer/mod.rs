//! Implementation of the MdfWriter struct split across several submodules

use std::io::{Write, Seek};

trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}
use std::collections::HashMap;

use crate::blocks::channel_block::ChannelBlock;
use crate::error::MdfError;
use crate::writer::mdf_writer::data::ChannelEncoder;

mod io;
mod init;
pub mod data;
mod vlsd;

pub use io::InMemorySink;

/// Helper structure tracking an open DTBLOCK during writing
struct OpenDataBlock {
    dg_id: String,
    dt_id: String,
    start_pos: u64,
    record_size: usize,
    record_count: u64,
    /// Total number of records written across all DT blocks for this group
    total_record_count: u64,
    channels: Vec<ChannelBlock>,
    dt_ids: Vec<String>,
    dt_positions: Vec<u64>,
    dt_sizes: Vec<u64>,
    /// Scratch buffer reused for record encoding
    record_buf: Vec<u8>,
    /// Template filled with constant values used to initialise each record
    record_template: Vec<u8>,
    /// Precomputed per-channel encoders
    encoders: Vec<ChannelEncoder>,
    /// Per-channel VLSD payload accumulator. `Some(buf)` for VLSD channels
    /// (channel_type == 1), `None` otherwise. The buffer holds
    /// the running [u32 length][bytes] stream that will be emitted as a ##SD
    /// block in `finish_data_block`.
    vlsd_payloads: Vec<Option<Vec<u8>>>,
    /// Writer-side channel IDs (cn_*) for VLSD channels, used to patch the
    /// `cn_data` link to the SD block in `finish_data_block`.
    vlsd_channel_ids: Vec<Option<String>>,
}


/// Writer for MDF blocks, ensuring 8-byte alignment and zero padding.
/// Tracks block positions and supports updating links at a later stage.
pub struct MdfWriter {
    file: Box<dyn WriteSeek>,
    offset: u64,
    block_positions: HashMap<String, u64>,
    open_dts: HashMap<String, OpenDataBlock>,
    /// In-memory VLSD payload buffers keyed by channel id. Each entry holds
    /// the concatenated `[u32 length][bytes]…` stream collected between
    /// `start_signal_data_block` and `finish_signal_data_block`. Buffers are
    /// flushed to ##SD blocks (chained via ##DL when large) on finish.
    sd_buffers: HashMap<String, Vec<u8>>,
    dt_counter: usize,
    last_dg: Option<String>,
    cg_to_dg: HashMap<String, String>,
    cg_offsets: HashMap<String, usize>,
    cg_channels: HashMap<String, Vec<ChannelBlock>>,
    /// Parallel to `cg_channels`: writer-side channel ids (cn_*) per channel
    /// group, in the same order. Used to look up VLSD channel ids when the
    /// open DT block emits its SD block.
    cg_channel_ids: HashMap<String, Vec<String>>,
    channel_map: HashMap<String, (String, usize)>,
    /// invalidation_bytes_nr configured for each channel group (captured from
    /// the `add_channel_group` closure). `start_data_block` includes these
    /// bytes (zero-filled) in the record layout so the declared record stride
    /// matches the written stride.
    cg_inval_bytes: HashMap<String, u32>,
}
