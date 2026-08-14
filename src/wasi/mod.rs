//! WASI Preview 2 (WebAssembly Component) bindings for mf4-rs.
//!
//! This module implements the `mf4` world defined in `wasi/wit/world.wit`,
//! exporting the reader / writer / index / ops interfaces as a WebAssembly
//! Component. It mirrors the name-based API philosophy of the Python
//! (`src/python.rs`) and wasm-bindgen (`src/wasm.rs`) bindings: navigation is
//! by group/channel **name**, with an optional `group` argument to
//! disambiguate duplicate channel names.
//!
//! It is compiled only when the `wasip2` feature is enabled and is intended
//! for the `wasm32-wasip2` target. Unlike the `wasm-bindgen` binding, a WASI
//! component has access to the host filesystem (via `std::fs` over WASI
//! preopens), so the reader/writer/index also expose `*-file` variants
//! alongside the in-memory (`bytes`/`fragments`) ones.
//!
//! The shared, target-neutral fragment machinery in [`crate::fragments`] backs
//! the lazy fragment-based index reads, exactly as it does for the
//! wasm-bindgen binding.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::api::mdf::MDF;
use crate::blocks::common::DataType;
use crate::block_layout::FileLayout;
use crate::error::MdfError;
use crate::fragments::{merge_ranges, FragmentRangeReader};
use crate::index::MdfIndex;
use crate::parsing::decoder::DecodedValue;
use crate::signal::Signal;
use crate::writer::{InMemorySink, MdfWriter};

wit_bindgen::generate!({
    world: "mf4",
    path: "wasi/wit",
});

use exports::mf4::core::index::{Guest as IndexGuest, GuestMdfIndex, MdfIndex as MdfIndexHandle};
use exports::mf4::core::ops::Guest as OpsGuest;
use exports::mf4::core::reader::{Guest as ReaderGuest, GuestMdf, Mdf as MdfHandle};
use exports::mf4::core::types::{
    ByteRange, ChannelInfo, DataType as WitDataType, DecodedValue as WitDecodedValue, GroupInfo,
    IndexGroupInfo, Signal as WitSignal,
};
use exports::mf4::core::writer::{Guest as WriterGuest, GuestMdfWriter};

/// Turn any crate error into the `string` error payload the WIT surfaces use.
fn e(err: MdfError) -> String {
    err.to_string()
}

// ---------------------------------------------------------------------------
// Type mapping helpers (native <-> WIT)
// ---------------------------------------------------------------------------

fn dt_to_wit(dt: &DataType) -> WitDataType {
    match dt {
        DataType::UnsignedIntegerLE => WitDataType::UnsignedIntegerLe,
        DataType::UnsignedIntegerBE => WitDataType::UnsignedIntegerBe,
        DataType::SignedIntegerLE => WitDataType::SignedIntegerLe,
        DataType::SignedIntegerBE => WitDataType::SignedIntegerBe,
        DataType::FloatLE => WitDataType::FloatLe,
        DataType::FloatBE => WitDataType::FloatBe,
        DataType::StringLatin1 => WitDataType::StringLatin1,
        DataType::StringUtf8 => WitDataType::StringUtf8,
        DataType::StringUtf16LE => WitDataType::StringUtf16Le,
        DataType::StringUtf16BE => WitDataType::StringUtf16Be,
        DataType::ByteArray => WitDataType::ByteArray,
        DataType::MimeSample => WitDataType::MimeSample,
        DataType::MimeStream => WitDataType::MimeStream,
        DataType::CanOpenDate => WitDataType::CanOpenDate,
        DataType::CanOpenTime => WitDataType::CanOpenTime,
        DataType::ComplexLE => WitDataType::ComplexLe,
        DataType::ComplexBE => WitDataType::ComplexBe,
        DataType::Unknown(_) => WitDataType::Unknown,
    }
}

/// Map a WIT data-type onto the native one, defaulting bit-count sensitive
/// callers to the LE variants. `unknown` becomes `Unknown(0)`.
fn dt_from_wit(dt: WitDataType) -> DataType {
    match dt {
        WitDataType::UnsignedIntegerLe => DataType::UnsignedIntegerLE,
        WitDataType::UnsignedIntegerBe => DataType::UnsignedIntegerBE,
        WitDataType::SignedIntegerLe => DataType::SignedIntegerLE,
        WitDataType::SignedIntegerBe => DataType::SignedIntegerBE,
        WitDataType::FloatLe => DataType::FloatLE,
        WitDataType::FloatBe => DataType::FloatBE,
        WitDataType::StringLatin1 => DataType::StringLatin1,
        WitDataType::StringUtf8 => DataType::StringUtf8,
        WitDataType::StringUtf16Le => DataType::StringUtf16LE,
        WitDataType::StringUtf16Be => DataType::StringUtf16BE,
        WitDataType::ByteArray => DataType::ByteArray,
        WitDataType::MimeSample => DataType::MimeSample,
        WitDataType::MimeStream => DataType::MimeStream,
        WitDataType::CanOpenDate => DataType::CanOpenDate,
        WitDataType::CanOpenTime => DataType::CanOpenTime,
        WitDataType::ComplexLe => DataType::ComplexLE,
        WitDataType::ComplexBe => DataType::ComplexBE,
        WitDataType::Unknown => DataType::Unknown(()),
    }
}

/// `true` for the text data types, which the writer stores as variable-length
/// (VLSD) channels rather than fixed-length record fields.
fn is_string_type(dt: &DataType) -> bool {
    matches!(
        dt,
        DataType::StringLatin1
            | DataType::StringUtf8
            | DataType::StringUtf16LE
            | DataType::StringUtf16BE
    )
}

fn dv_to_wit(v: &DecodedValue) -> WitDecodedValue {
    match v {
        DecodedValue::UnsignedInteger(n) => WitDecodedValue::UnsignedInteger(*n),
        DecodedValue::SignedInteger(n) => WitDecodedValue::SignedInteger(*n),
        DecodedValue::Float(f) => WitDecodedValue::Float(*f),
        DecodedValue::String(s) => WitDecodedValue::String(s.clone()),
        DecodedValue::ByteArray(b) => WitDecodedValue::ByteArray(b.clone()),
        DecodedValue::MimeSample(b) => WitDecodedValue::MimeSample(b.clone()),
        DecodedValue::MimeStream(b) => WitDecodedValue::MimeStream(b.clone()),
        DecodedValue::Unknown => WitDecodedValue::Unknown,
    }
}

fn dv_from_wit(v: WitDecodedValue) -> DecodedValue {
    match v {
        WitDecodedValue::UnsignedInteger(n) => DecodedValue::UnsignedInteger(n),
        WitDecodedValue::SignedInteger(n) => DecodedValue::SignedInteger(n),
        WitDecodedValue::Float(f) => DecodedValue::Float(f),
        WitDecodedValue::String(s) => DecodedValue::String(s),
        WitDecodedValue::ByteArray(b) => DecodedValue::ByteArray(b),
        WitDecodedValue::MimeSample(b) => DecodedValue::MimeSample(b),
        WitDecodedValue::MimeStream(b) => DecodedValue::MimeStream(b),
        WitDecodedValue::Unknown => DecodedValue::Unknown,
    }
}

fn signal_to_wit(sig: Signal) -> WitSignal {
    WitSignal {
        name: sig.name,
        unit: sig.unit,
        timestamps: sig.timestamps,
        values: sig
            .values
            .iter()
            .map(|v| v.as_ref().map(dv_to_wit))
            .collect(),
    }
}

/// Assemble a whole-file [`FragmentRangeReader`] from parallel WIT
/// `ranges`/`fragments` lists.
fn fragment_reader(
    ranges: &[ByteRange],
    fragments: Vec<Vec<u8>>,
) -> Result<FragmentRangeReader, String> {
    if ranges.len() != fragments.len() {
        return Err(format!(
            "ranges ({}) and fragments ({}) must be parallel lists of equal length",
            ranges.len(),
            fragments.len()
        ));
    }
    let pairs: Vec<(u64, Vec<u8>)> = ranges
        .iter()
        .map(|r| r.offset)
        .zip(fragments)
        .collect();
    FragmentRangeReader::from_pairs(pairs).map_err(e)
}

fn ranges_to_wit(ranges: Vec<(u64, u64)>) -> Vec<ByteRange> {
    ranges
        .into_iter()
        .map(|(offset, length)| ByteRange { offset, length })
        .collect()
}

// ---------------------------------------------------------------------------
// Component root + interface wiring
// ---------------------------------------------------------------------------

struct Component;

impl ReaderGuest for Component {
    type Mdf = MdfRes;
}

impl WriterGuest for Component {
    type MdfWriter = MdfWriterRes;
}

impl IndexGuest for Component {
    type MdfIndex = MdfIndexRes;
}

// ---------------------------------------------------------------------------
// reader: mdf resource
// ---------------------------------------------------------------------------

/// Backing state for the exported `mdf` reader resource. Holds the parsed
/// [`MDF`] plus a copy of the source bytes so `file-layout-json` can describe
/// the physical block layout without re-reading the file.
struct MdfRes {
    mdf: MDF,
    bytes: Vec<u8>,
}

impl GuestMdf for MdfRes {
    fn from_bytes(data: Vec<u8>) -> Result<MdfHandle, String> {
        let mdf = MDF::from_bytes(data.clone()).map_err(e)?;
        Ok(MdfHandle::new(MdfRes { mdf, bytes: data }))
    }

    fn from_file(path: String) -> Result<MdfHandle, String> {
        let data = std::fs::read(&path).map_err(|err| err.to_string())?;
        let mdf = MDF::from_bytes(data.clone()).map_err(e)?;
        Ok(MdfHandle::new(MdfRes { mdf, bytes: data }))
    }

    fn groups(&self) -> Result<Vec<GroupInfo>, String> {
        let mut groups = Vec::new();
        for g in self.mdf.channel_groups() {
            let mut channels = Vec::new();
            for ch in g.channels() {
                let block = ch.block();
                channels.push(ChannelInfo {
                    name: ch.name().map_err(e)?,
                    unit: ch.unit().map_err(e)?,
                    comment: ch.comment().map_err(e)?,
                    data_type: dt_to_wit(&block.data_type),
                    is_master: block.channel_type == 2,
                    bit_count: block.bit_count,
                });
            }
            groups.push(GroupInfo {
                name: g.name().map_err(e)?,
                comment: g.comment().map_err(e)?,
                record_count: g.raw_channel_group().block.cycles_nr,
                channels,
            });
        }
        Ok(groups)
    }

    fn channel_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for g in self.mdf.channel_groups() {
            for ch in g.channels() {
                if let Ok(Some(n)) = ch.name() {
                    names.push(n);
                }
            }
        }
        names
    }

    fn values(&self, name: String, group: Option<String>) -> Result<Vec<f64>, String> {
        for g in self.mdf.channel_groups() {
            if let Some(gn) = group.as_deref() {
                if g.name().map_err(e)?.as_deref() != Some(gn) {
                    continue;
                }
            }
            for ch in g.channels() {
                if ch.name().map_err(e)?.as_deref() == Some(name.as_str()) {
                    return ch.values_as_f64().map_err(e);
                }
            }
        }
        Err(match group {
            Some(gn) => format!("Channel '{}' not found in group '{}'", name, gn),
            None => format!("Channel '{}' not found", name),
        })
    }

    fn read(&self, name: String, group: Option<String>) -> Result<WitSignal, String> {
        let signal = match group.as_deref() {
            Some(gn) => match self.mdf.group(gn) {
                Some(g) => g.signal(&name).map_err(e)?,
                None => return Err(format!("Channel group '{}' not found", gn)),
            },
            None => self.mdf.signal(&name).map_err(e)?,
        };
        let signal = signal.ok_or_else(|| match group {
            Some(gn) => format!("Channel '{}' not found in group '{}'", name, gn),
            None => format!("Channel '{}' not found", name),
        })?;
        Ok(signal_to_wit(signal))
    }

    fn start_time_ns(&self) -> Option<u64> {
        self.mdf.start_time_ns()
    }

    fn file_layout_json(&self) -> Result<String, String> {
        let layout = FileLayout::from_bytes(&self.bytes).map_err(e)?;
        layout.to_json().map_err(e)
    }
}

// ---------------------------------------------------------------------------
// writer: mdf-writer resource
// ---------------------------------------------------------------------------

/// Mutable state behind the `mdf-writer` resource. WIT resource methods take
/// `&self`, so the actual writer lives inside a [`RefCell`]; the shared
/// [`InMemorySink`] lets `finalize` recover the finished bytes after the
/// writer consumes itself.
struct WriterState {
    writer: Option<MdfWriter>,
    sink: InMemorySink,
    channel_groups: HashMap<String, String>,
    channels: HashMap<String, String>,
    last_channels: HashMap<String, String>,
    channel_types: HashMap<String, Vec<DataType>>,
    next_id: usize,
}

impl WriterState {
    fn writer_mut(&mut self) -> Result<&mut MdfWriter, String> {
        self.writer
            .as_mut()
            .ok_or_else(|| "Writer has been finalized".to_string())
    }

    fn add_channel_with_bits(
        &mut self,
        group_id: &str,
        name: &str,
        data_type: DataType,
        bit_count: u32,
    ) -> Result<String, String> {
        let cg_id = self
            .channel_groups
            .get(group_id)
            .ok_or_else(|| "Channel group not found".to_string())?
            .clone();
        let prev_channel_id = self
            .last_channels
            .get(group_id)
            .and_then(|id| self.channels.get(id))
            .cloned();
        let dt = data_type.clone();
        let name_owned = name.to_string();
        let ch_id = self
            .writer_mut()?
            .add_channel(&cg_id, prev_channel_id.as_deref(), |ch| {
                ch.data_type = dt.clone();
                ch.name = Some(name_owned.clone());
                ch.bit_count = bit_count;
            })
            .map_err(e)?;

        let logical = format!("ch_{}", self.next_id);
        self.next_id += 1;
        self.channels.insert(logical.clone(), ch_id);
        self.last_channels.insert(group_id.to_string(), logical.clone());
        self.channel_types
            .entry(group_id.to_string())
            .or_default()
            .push(data_type);
        Ok(logical)
    }

    fn set_time_channel(&mut self, channel_id: &str) -> Result<(), String> {
        let ch_id = self
            .channels
            .get(channel_id)
            .ok_or_else(|| "Channel not found".to_string())?
            .clone();
        self.writer_mut()?.set_time_channel(&ch_id).map_err(e)?;
        Ok(())
    }
}

/// The exported `mdf-writer` resource.
struct MdfWriterRes {
    inner: RefCell<WriterState>,
}

impl GuestMdfWriter for MdfWriterRes {
    fn new() -> Self {
        let sink = InMemorySink::new();
        let writer = MdfWriter::new_from_writer(sink.clone());
        MdfWriterRes {
            inner: RefCell::new(WriterState {
                writer: Some(writer),
                sink,
                channel_groups: HashMap::new(),
                channels: HashMap::new(),
                last_channels: HashMap::new(),
                channel_types: HashMap::new(),
                next_id: 0,
            }),
        }
    }

    fn init_mdf_file(&self) -> Result<(), String> {
        self.inner.borrow_mut().writer_mut()?.init_mdf_file().map_err(e)?;
        Ok(())
    }

    fn set_start_time(&self, abs_time_ns: u64) -> Result<(), String> {
        self.inner
            .borrow_mut()
            .writer_mut()?
            .set_start_time(abs_time_ns, 0, 0, 0, 0)
            .map_err(e)?;
        Ok(())
    }

    fn add_channel_group(&self, name: Option<String>) -> Result<String, String> {
        let mut st = self.inner.borrow_mut();
        let cg_id = st.writer_mut()?.add_channel_group(None, |_cg| {}).map_err(e)?;
        if let Some(n) = &name {
            st.writer_mut()?.set_channel_group_name(&cg_id, n).map_err(e)?;
        }
        let logical = format!("cg_{}", st.next_id);
        st.next_id += 1;
        st.channel_groups.insert(logical.clone(), cg_id);
        st.channel_types.insert(logical.clone(), Vec::new());
        Ok(logical)
    }

    fn add_channel(
        &self,
        group_id: String,
        name: String,
        dt: WitDataType,
    ) -> Result<String, String> {
        let native = dt_from_wit(dt);
        if is_string_type(&native) {
            return Err(
                "string data types are variable-length in this writer; use add-string-channel"
                    .to_string(),
            );
        }
        let bits = native.default_bits();
        self.inner
            .borrow_mut()
            .add_channel_with_bits(&group_id, &name, native, bits)
    }

    fn add_string_channel(&self, group_id: String, name: String) -> Result<String, String> {
        let mut st = self.inner.borrow_mut();
        let cg_id = st
            .channel_groups
            .get(&group_id)
            .ok_or_else(|| "Channel group not found".to_string())?
            .clone();
        let prev_channel_id = st
            .last_channels
            .get(&group_id)
            .and_then(|id| st.channels.get(id))
            .cloned();
        let name_owned = name.clone();
        let ch_id = st
            .writer_mut()?
            .add_channel(&cg_id, prev_channel_id.as_deref(), |ch| {
                ch.data_type = DataType::StringUtf8;
                ch.name = Some(name_owned.clone());
                ch.channel_type = 1; // VLSD
                ch.data = 1; // routed to the SD-block writer; patched on finish
                ch.bit_count = 64; // record slot holds a u64 offset
            })
            .map_err(e)?;

        let logical = format!("ch_{}", st.next_id);
        st.next_id += 1;
        st.channels.insert(logical.clone(), ch_id);
        st.last_channels.insert(group_id.clone(), logical.clone());
        st.channel_types
            .entry(group_id)
            .or_default()
            .push(DataType::StringUtf8);
        Ok(logical)
    }

    fn add_time_channel(&self, group_id: String, name: String) -> Result<String, String> {
        let mut st = self.inner.borrow_mut();
        let ch_id = st.add_channel_with_bits(&group_id, &name, DataType::FloatLE, 64)?;
        st.set_time_channel(&ch_id)?;
        Ok(ch_id)
    }

    fn add_float_channel(&self, group_id: String, name: String) -> Result<String, String> {
        self.inner
            .borrow_mut()
            .add_channel_with_bits(&group_id, &name, DataType::FloatLE, 64)
    }

    fn add_float32_channel(&self, group_id: String, name: String) -> Result<String, String> {
        self.inner
            .borrow_mut()
            .add_channel_with_bits(&group_id, &name, DataType::FloatLE, 32)
    }

    fn add_int_channel(&self, group_id: String, name: String) -> Result<String, String> {
        self.inner
            .borrow_mut()
            .add_channel_with_bits(&group_id, &name, DataType::UnsignedIntegerLE, 64)
    }

    fn add_sint_channel(&self, group_id: String, name: String) -> Result<String, String> {
        self.inner
            .borrow_mut()
            .add_channel_with_bits(&group_id, &name, DataType::SignedIntegerLE, 64)
    }

    fn set_time_channel(&self, channel_id: String) -> Result<(), String> {
        self.inner.borrow_mut().set_time_channel(&channel_id)
    }

    fn start_data_block(&self, group_id: String) -> Result<(), String> {
        let mut st = self.inner.borrow_mut();
        let cg_id = st
            .channel_groups
            .get(&group_id)
            .ok_or_else(|| "Channel group not found".to_string())?
            .clone();
        st.writer_mut()?.start_data_block_for_cg(&cg_id, 0).map_err(e)?;
        Ok(())
    }

    fn write_record(
        &self,
        group_id: String,
        values: Vec<WitDecodedValue>,
    ) -> Result<(), String> {
        let mut st = self.inner.borrow_mut();
        let cg_id = st
            .channel_groups
            .get(&group_id)
            .ok_or_else(|| "Channel group not found".to_string())?
            .clone();
        let expected = st
            .channel_types
            .get(&group_id)
            .ok_or_else(|| "Channel group not found".to_string())?
            .len();
        if values.len() != expected {
            return Err(format!(
                "expected {} values for this group, got {}",
                expected,
                values.len()
            ));
        }
        let rust_values: Vec<DecodedValue> = values.into_iter().map(dv_from_wit).collect();
        st.writer_mut()?.write_record(&cg_id, &rust_values).map_err(e)?;
        Ok(())
    }

    fn finish_data_block(&self, group_id: String) -> Result<(), String> {
        let mut st = self.inner.borrow_mut();
        let cg_id = st
            .channel_groups
            .get(&group_id)
            .ok_or_else(|| "Channel group not found".to_string())?
            .clone();
        st.writer_mut()?.finish_data_block(&cg_id).map_err(e)?;
        Ok(())
    }

    fn finalize(&self) -> Result<Vec<u8>, String> {
        let mut st = self.inner.borrow_mut();
        let writer = st
            .writer
            .take()
            .ok_or_else(|| "Writer already finalized".to_string())?;
        writer.finalize().map_err(e)?;
        Ok(st.sink.to_vec())
    }

    fn finalize_to_file(&self, path: String) -> Result<(), String> {
        let bytes = {
            let mut st = self.inner.borrow_mut();
            let writer = st
                .writer
                .take()
                .ok_or_else(|| "Writer already finalized".to_string())?;
            writer.finalize().map_err(e)?;
            st.sink.to_vec()
        };
        std::fs::write(&path, bytes).map_err(|err| err.to_string())
    }
}

// ---------------------------------------------------------------------------
// index: mdf-index resource
// ---------------------------------------------------------------------------

/// The exported `mdf-index` resource. Wraps a self-contained [`MdfIndex`];
/// fragment reads bind a fresh [`FragmentRangeReader`] per call, so the index
/// itself is only ever borrowed immutably.
struct MdfIndexRes {
    index: MdfIndex,
}

impl MdfIndexRes {
    fn locate(&self, name: &str, group: Option<&str>) -> Result<(usize, usize), String> {
        let found = match group {
            Some(gn) => self.index.locate_in(gn, name),
            None => self.index.locate(name),
        };
        found.ok_or_else(|| match group {
            Some(gn) => format!("Channel '{}' not found in group '{}'", name, gn),
            None => format!("Channel '{}' not found", name),
        })
    }

    /// Full data-section byte ranges for the group owning `name` (fixed records
    /// plus any VLSD `##SD` fragments), merged. Mirrors the wasm binding's
    /// `signal_byte_ranges`.
    fn signal_ranges(&self, name: &str, group: Option<&str>) -> Result<Vec<(u64, u64)>, String> {
        let (g, c) = self.locate(name, group)?;
        let grp = &self.index.channel_groups[g];
        let channel = &grp.channels[c];
        let mut ranges =
            Vec::with_capacity(grp.data_blocks.len() + channel.vlsd_data_blocks.len());
        for db in grp.data_blocks.iter().chain(channel.vlsd_data_blocks.iter()) {
            if db.is_compressed {
                return Err(
                    "compressed (##DZ) data blocks are not supported for fragment reads"
                        .to_string(),
                );
            }
            let data_len = db.size.checked_sub(24).ok_or_else(|| {
                format!(
                    "invalid index: data block at offset {} has size {} (smaller than the \
                     24-byte block header)",
                    db.file_offset, db.size
                )
            })?;
            ranges.push((db.file_offset + 24, data_len));
        }
        Ok(merge_ranges(ranges))
    }
}

impl GuestMdfIndex for MdfIndexRes {
    fn from_bytes(data: Vec<u8>) -> Result<MdfIndexHandle, String> {
        let index = MdfIndex::from_bytes(data).map_err(e)?;
        Ok(MdfIndexHandle::new(MdfIndexRes { index }))
    }

    fn from_file(path: String) -> Result<MdfIndexHandle, String> {
        let data = std::fs::read(&path).map_err(|err| err.to_string())?;
        let index = MdfIndex::from_bytes(data).map_err(e)?;
        Ok(MdfIndexHandle::new(MdfIndexRes { index }))
    }

    fn from_json(json: String) -> Result<MdfIndexHandle, String> {
        let index = MdfIndex::from_json(&json).map_err(e)?;
        Ok(MdfIndexHandle::new(MdfIndexRes { index }))
    }

    fn load_from_file(path: String) -> Result<MdfIndexHandle, String> {
        let index = MdfIndex::load_from_file(&path).map_err(e)?;
        Ok(MdfIndexHandle::new(MdfIndexRes { index }))
    }

    fn to_json(&self) -> Result<String, String> {
        self.index.to_json().map_err(e)
    }

    fn save_to_file(&self, path: String) -> Result<(), String> {
        self.index.save_to_file(&path).map_err(e)
    }

    fn validate(&self) -> Result<(), String> {
        self.index.validate().map_err(e)
    }

    fn groups(&self) -> Result<Vec<IndexGroupInfo>, String> {
        Ok(self
            .index
            .groups()
            .iter()
            .map(|g| IndexGroupInfo {
                name: g.name.clone(),
                record_count: g.record_count,
                channel_names: g.channel_names().into_iter().map(String::from).collect(),
                master_channel: g.master_channel().and_then(|c| c.name.clone()),
            })
            .collect())
    }

    fn channel_names(&self) -> Vec<String> {
        self.index.channel_names().into_iter().map(String::from).collect()
    }

    fn file_size(&self) -> u64 {
        self.index.file_size
    }

    fn byte_ranges(&self, name: String, group: Option<String>) -> Result<Vec<ByteRange>, String> {
        let ranges = match group.as_deref() {
            Some(gn) => self.index.byte_ranges_in(gn, &name).map_err(e)?,
            None => self.index.byte_ranges(&name).map_err(e)?,
        };
        Ok(ranges_to_wit(ranges))
    }

    fn byte_ranges_for_records(
        &self,
        name: String,
        start_record: u64,
        record_count: u64,
        group: Option<String>,
    ) -> Result<Vec<ByteRange>, String> {
        let ranges = match group.as_deref() {
            Some(gn) => {
                let (g, c) = self.locate(&name, Some(gn))?;
                self.index
                    .get_channel_byte_ranges_for_records(g, c, start_record, record_count)
                    .map_err(e)?
            }
            None => self
                .index
                .byte_ranges_for_records(&name, start_record, record_count)
                .map_err(e)?,
        };
        Ok(ranges_to_wit(ranges))
    }

    fn signal_byte_ranges(
        &self,
        name: String,
        group: Option<String>,
    ) -> Result<Vec<ByteRange>, String> {
        let ranges = self.signal_ranges(&name, group.as_deref())?;
        Ok(ranges_to_wit(ranges))
    }

    fn values_from_fragments(
        &self,
        name: String,
        group: Option<String>,
        ranges: Vec<ByteRange>,
        fragments: Vec<Vec<u8>>,
    ) -> Result<Vec<f64>, String> {
        let reader = fragment_reader(&ranges, fragments)?;
        let mut bound = self.index.open(reader);
        match group.as_deref() {
            Some(gn) => bound.values_f64_in(gn, &name).map_err(e),
            None => bound.values_f64(&name).map_err(e),
        }
    }

    fn read_from_fragments(
        &self,
        name: String,
        group: Option<String>,
        ranges: Vec<ByteRange>,
        fragments: Vec<Vec<u8>>,
    ) -> Result<WitSignal, String> {
        let reader = fragment_reader(&ranges, fragments)?;
        let mut bound = self.index.open(reader);
        let signal = match group.as_deref() {
            Some(gn) => bound.signal_in(gn, &name).map_err(e)?,
            None => bound.signal(&name).map_err(e)?,
        };
        Ok(signal_to_wit(signal))
    }

    fn values_from_file(
        &self,
        name: String,
        group: Option<String>,
        path: String,
    ) -> Result<Vec<f64>, String> {
        let data = std::fs::read(&path).map_err(|err| err.to_string())?;
        let reader = FragmentRangeReader::from_pairs(vec![(0, data)]).map_err(e)?;
        let mut bound = self.index.open(reader);
        match group.as_deref() {
            Some(gn) => bound.values_f64_in(gn, &name).map_err(e),
            None => bound.values_f64(&name).map_err(e),
        }
    }

    fn read_from_file(
        &self,
        name: String,
        group: Option<String>,
        path: String,
    ) -> Result<WitSignal, String> {
        let data = std::fs::read(&path).map_err(|err| err.to_string())?;
        let reader = FragmentRangeReader::from_pairs(vec![(0, data)]).map_err(e)?;
        let mut bound = self.index.open(reader);
        let signal = match group.as_deref() {
            Some(gn) => bound.signal_in(gn, &name).map_err(e)?,
            None => bound.signal(&name).map_err(e)?,
        };
        Ok(signal_to_wit(signal))
    }
}

// ---------------------------------------------------------------------------
// ops: free functions
// ---------------------------------------------------------------------------

impl OpsGuest for Component {
    fn cut_by_time(input: Vec<u8>, start: f64, end: f64) -> Result<Vec<u8>, String> {
        crate::cut::cut_mdf_by_time_bytes(&input, start, end).map_err(e)
    }

    fn cut_by_utc(input: Vec<u8>, start_ns: u64, end_ns: u64) -> Result<Vec<u8>, String> {
        crate::cut::cut_mdf_by_utc_ns_bytes(&input, start_ns as i64, end_ns as i64).map_err(e)
    }

    fn merge(first: Vec<u8>, second: Vec<u8>) -> Result<Vec<u8>, String> {
        crate::merge::merge_files_bytes(&first, &second).map_err(e)
    }

    fn cut_by_time_file(
        input: String,
        output: String,
        start: f64,
        end: f64,
    ) -> Result<(), String> {
        let data = std::fs::read(&input).map_err(|err| err.to_string())?;
        let out = crate::cut::cut_mdf_by_time_bytes(&data, start, end).map_err(e)?;
        std::fs::write(&output, out).map_err(|err| err.to_string())
    }

    fn cut_by_utc_file(
        input: String,
        output: String,
        start_ns: u64,
        end_ns: u64,
    ) -> Result<(), String> {
        let data = std::fs::read(&input).map_err(|err| err.to_string())?;
        let out = crate::cut::cut_mdf_by_utc_ns_bytes(&data, start_ns as i64, end_ns as i64)
            .map_err(e)?;
        std::fs::write(&output, out).map_err(|err| err.to_string())
    }

    fn merge_file(output: String, first: String, second: String) -> Result<(), String> {
        let a = std::fs::read(&first).map_err(|err| err.to_string())?;
        let b = std::fs::read(&second).map_err(|err| err.to_string())?;
        let out = crate::merge::merge_files_bytes(&a, &b).map_err(e)?;
        std::fs::write(&output, out).map_err(|err| err.to_string())
    }
}

export!(Component);
