// Low level file and block handling utilities for MdfWriter
use super::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::rc::Rc;
use byteorder::{LittleEndian, WriteBytesExt};

/// A shareable, in-memory `Write + Seek` sink for [`MdfWriter::new_from_writer`].
///
/// Clone the handle, hand one clone to the writer, and recover the finished
/// file bytes from the other after [`MdfWriter::finalize`]. This is the
/// portable way to produce an MDF file entirely in memory (used by the
/// `*_bytes` cutting/merging entry points and the wasm/WASI bindings). It is
/// single-threaded (`Rc`-based), which suits the wasm targets and synchronous
/// native use.
#[derive(Clone, Default)]
pub struct InMemorySink(Rc<RefCell<Cursor<Vec<u8>>>>);

impl InMemorySink {
    /// Create an empty in-memory sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Copy out the bytes written so far.
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.borrow().get_ref().clone()
    }
}

impl Write for InMemorySink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.borrow_mut().flush()
    }
}

impl Seek for InMemorySink {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.0.borrow_mut().seek(pos)
    }
}

#[cfg(not(target_arch = "wasm32"))]
use std::fs::File;
#[cfg(not(target_arch = "wasm32"))]
use std::io::BufWriter;
#[cfg(not(target_arch = "wasm32"))]
use memmap2::MmapMut;

#[cfg(not(target_arch = "wasm32"))]
struct MmapWriter {
    mmap: MmapMut,
    pos: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl MmapWriter {
    fn new(path: &str, size: usize) -> Result<Self, MdfError> {
        use std::fs::OpenOptions;
        let file = OpenOptions::new().read(true).write(true).create(true).open(path)?;
        file.set_len(size as u64)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        Ok(MmapWriter { mmap, pos: 0 })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Write for MmapWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let end = self.pos + buf.len();
        if end > self.mmap.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::WriteZero, "mmap overflow"));
        }
        self.mmap[self.pos..end].copy_from_slice(buf);
        self.pos = end;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.mmap.flush()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Seek for MmapWriter {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos: i64 = match pos {
            SeekFrom::Start(x) => x as i64,
            SeekFrom::End(x) => self.mmap.len() as i64 + x,
            SeekFrom::Current(x) => self.pos as i64 + x,
        };
        if new_pos < 0 || new_pos as usize > self.mmap.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, "invalid seek"));
        }
        self.pos = new_pos as usize;
        Ok(self.pos as u64)
    }
}

impl MdfWriter {
    /// Creates a new MdfWriter from any `Write + Seek` backend.
    ///
    /// This is the only constructor available on `wasm32-unknown-unknown`.
    /// On native targets you can pass a `std::io::Cursor<Vec<u8>>` to produce
    /// an in-memory MDF file, or a `BufWriter<File>` for on-disk output.
    pub fn new_from_writer(w: impl Write + Seek + 'static) -> Self {
        MdfWriter {
            file: Box::new(w),
            offset: 0,
            block_positions: HashMap::new(),
            open_dts: HashMap::new(),
            sd_buffers: HashMap::new(),
            dt_counter: 0,
            last_dg: None,
            cg_to_dg: HashMap::new(),
            cg_offsets: HashMap::new(),
            cg_channels: HashMap::new(),
            cg_channel_ids: HashMap::new(),
            channel_map: HashMap::new(),
            cg_inval_bytes: HashMap::new(),
        }
    }

    /// Creates a new MdfWriter for the given file path using a 1 MB internal
    /// buffer. Use [`new_with_capacity`] to customize the buffer size.
    ///
    /// Not available on `wasm32-unknown-unknown`; use [`new_from_writer`] instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(path: &str) -> Result<Self, MdfError> {
        Self::new_with_capacity(path, 1_048_576)
    }

    /// Creates a new MdfWriter with the specified `BufWriter` capacity.
    ///
    /// Not available on `wasm32-unknown-unknown`; use [`new_from_writer`] instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_with_capacity(path: &str, capacity: usize) -> Result<Self, MdfError> {
        let file = File::create(path)?;
        let file = BufWriter::with_capacity(capacity, file);
        Ok(MdfWriter {
            file: Box::new(file),
            offset: 0,
            block_positions: HashMap::new(),
            open_dts: HashMap::new(),
            sd_buffers: HashMap::new(),
            dt_counter: 0,
            last_dg: None,
            cg_to_dg: HashMap::new(),
            cg_offsets: HashMap::new(),
            cg_channels: HashMap::new(),
            cg_channel_ids: HashMap::new(),
            channel_map: HashMap::new(),
            cg_inval_bytes: HashMap::new(),
        })
    }

    /// Creates a new MdfWriter backed by a memory-mapped file of the given size.
    ///
    /// Not available on `wasm32-unknown-unknown`; use [`new_from_writer`] instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_mmap(path: &str, size: usize) -> Result<Self, MdfError> {
        let writer = MmapWriter::new(path, size)?;
        Ok(MdfWriter {
            file: Box::new(writer),
            offset: 0,
            block_positions: HashMap::new(),
            open_dts: HashMap::new(),
            sd_buffers: HashMap::new(),
            dt_counter: 0,
            last_dg: None,
            cg_to_dg: HashMap::new(),
            cg_offsets: HashMap::new(),
            cg_channels: HashMap::new(),
            cg_channel_ids: HashMap::new(),
            channel_map: HashMap::new(),
            cg_inval_bytes: HashMap::new(),
        })
    }

    /// Writes a block to the file, aligning to 8 bytes and zero-padding as needed.
    /// Returns the starting offset of the block in the file.
    pub fn write_block(&mut self, block_bytes: &[u8]) -> Result<u64, MdfError> {
        let align = (8 - (self.offset % 8)) % 8;
        if align != 0 {
            let padding = vec![0u8; align as usize];
            self.file.write_all(&padding)?;
            self.offset += align;
        }

        self.file.write_all(block_bytes)?;
        let block_start = self.offset;
        self.offset += block_bytes.len() as u64;
        Ok(block_start)
    }

    /// Writes a block to the file and tracks its position with the given ID.
    pub fn write_block_with_id(&mut self, block_bytes: &[u8], block_id: &str) -> Result<u64, MdfError> {
        let block_start = self.write_block(block_bytes)?;
        self.block_positions.insert(block_id.to_string(), block_start);
        Ok(block_start)
    }

    /// Retrieves the file position of a previously written block.
    pub fn get_block_position(&self, block_id: &str) -> Option<u64> {
        self.block_positions.get(block_id).copied()
    }

    /// Updates a link (u64 address) at a specific offset in the file.
    pub fn update_link(&mut self, offset: u64, address: u64) -> Result<(), MdfError> {
        let current_pos = self.offset;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_u64::<LittleEndian>(address)?;
        self.file.seek(SeekFrom::Start(current_pos))?;
        Ok(())
    }

    /// Updates a link using block IDs instead of raw offsets.
    pub fn update_block_link(&mut self, source_id: &str, link_offset: u64, target_id: &str) -> Result<(), MdfError> {
        let source_pos = self.get_block_position(source_id)
            .ok_or_else(|| MdfError::BlockLinkError(format!("Source block '{}' not found", source_id)))?;
        let target_pos = self.get_block_position(target_id)
            .ok_or_else(|| MdfError::BlockLinkError(format!("Target block '{}' not found", target_id)))?;
        let link_pos = source_pos + link_offset;
        self.update_link(link_pos, target_pos)
    }

    fn update_u32(&mut self, offset: u64, value: u32) -> Result<(), MdfError> {
        let current_pos = self.offset;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_u32::<LittleEndian>(value)?;
        self.file.seek(SeekFrom::Start(current_pos))?;
        Ok(())
    }

    fn update_u64(&mut self, offset: u64, value: u64) -> Result<(), MdfError> {
        let current_pos = self.offset;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_u64::<LittleEndian>(value)?;
        self.file.seek(SeekFrom::Start(current_pos))?;
        Ok(())
    }

    fn update_u8(&mut self, offset: u64, value: u8) -> Result<(), MdfError> {
        let current_pos = self.offset;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_u8(value)?;
        self.file.seek(SeekFrom::Start(current_pos))?;
        Ok(())
    }

    pub(super) fn update_block_u32(&mut self, block_id: &str, field_offset: u64, value: u32) -> Result<(), MdfError> {
        let block_pos = self.get_block_position(block_id)
            .ok_or_else(|| MdfError::BlockLinkError(format!("Block '{}' not found", block_id)))?;
        self.update_u32(block_pos + field_offset, value)
    }

    pub(super) fn update_block_u8(&mut self, block_id: &str, field_offset: u64, value: u8) -> Result<(), MdfError> {
        let block_pos = self.get_block_position(block_id)
            .ok_or_else(|| MdfError::BlockLinkError(format!("Block '{}' not found", block_id)))?;
        self.update_u8(block_pos + field_offset, value)
    }

    pub(super) fn update_block_u64(&mut self, block_id: &str, field_offset: u64, value: u64) -> Result<(), MdfError> {
        let block_pos = self.get_block_position(block_id)
            .ok_or_else(|| MdfError::BlockLinkError(format!("Block '{}' not found", block_id)))?;
        self.update_u64(block_pos + field_offset, value)
    }

    /// Returns the current file offset (for block address calculation).
    pub fn offset(&self) -> u64 { self.offset }

    /// Finalizes the file (flushes all data to disk).
    pub fn finalize(mut self) -> Result<(), MdfError> {
        self.file.flush()?;
        Ok(())
    }
}
