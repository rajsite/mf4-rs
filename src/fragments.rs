//! Shared, target-neutral fragment-backed byte-range readers.
//!
//! This module holds the fragment machinery used by both the `wasm`
//! (`wasm-bindgen`) binding and the `wasip2` (WebAssembly Component) binding
//! to serve [`ByteRangeReader`] requests from caller-provided file fragments.
//! It has no dependency on `wasm-bindgen`/`js-sys`; the JS/WIT marshalling
//! wrappers live in the respective binding modules.
//!
//! The core pieces are:
//! - [`FragmentStore`] — a sorted set of `(offset, bytes)` fragments with
//!   subquadratic stabbing-query reads,
//! - [`FragmentRangeReader`] — a [`ByteRangeReader`] that errors on any
//!   uncovered range,
//! - [`RecordingRangeReader`] — a probing [`ByteRangeReader`] that records the
//!   gaps it cannot serve (drives incremental, range-fetched index builds),
//! - [`merge_ranges`] — coalesces overlapping/adjacent ranges into a minimal
//!   cover.

use crate::error::MdfError;
use crate::index::ByteRangeReader;

/// Sort and merge overlapping/adjacent byte ranges into a minimal cover.
pub(crate) fn merge_ranges(mut ranges: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    if ranges.is_empty() {
        return ranges;
    }
    ranges.sort_by_key(|r| r.0);
    let mut out: Vec<(u64, u64)> = Vec::with_capacity(ranges.len());
    for (offset, length) in ranges {
        let end = offset + length;
        if let Some(last) = out.last_mut() {
            let last_end = last.0 + last.1;
            if offset <= last_end {
                if end > last_end {
                    last.1 = end - last.0;
                }
                continue;
            }
        }
        out.push((offset, length));
    }
    out
}

/// Narrow a `u64` byte count/offset to `usize`, erroring instead of truncating
/// on a 32-bit (wasm) target where the value exceeds `usize::MAX`.
pub(crate) fn u64_to_usize(v: u64) -> Result<usize, MdfError> {
    usize::try_from(v).map_err(|_| {
        MdfError::BlockSerializationError(format!("byte value {} exceeds addressable range", v))
    })
}

/// End offset of a fragment, erroring on overflow instead of wrapping.
fn fragment_end(fstart: u64, len: usize) -> Result<u64, MdfError> {
    fstart.checked_add(len as u64).ok_or_else(|| {
        MdfError::BlockSerializationError("fragment offset + length overflows u64".to_string())
    })
}

/// Outcome of trying to serve a read from a set of file fragments.
pub(crate) enum Coverage {
    /// The request was fully covered; here are the assembled bytes.
    Full(Vec<u8>),
    /// A gap was found: `offset`/`length` describe the first still-missing
    /// sub-range of the request.
    Gap { offset: u64, length: u64 },
}

/// A store of file fragments — `(absolute_offset, bytes)` pairs, possibly
/// overlapping or exactly adjacent — kept sorted by `offset` so a read is
/// served by two binary searches (`partition_point`) instead of scanning
/// every stored fragment. This is what makes repeated reads against a
/// steadily growing fragment set (an incremental index build accumulating
/// fetched bytes across many passes) subquadratic: each `push` inserts once,
/// each `assemble`/read is `O(log F + w)` where `w` is the number of
/// fragments actually overlapping the request (typically tiny), not `O(F)`.
///
/// Alongside the sorted fragments, `running_max_end[i]` tracks the largest
/// end offset among `fragments[..=i]`. Because `fragments` is sorted by start
/// and `running_max_end` is therefore monotonically non-decreasing, binary
/// searching it finds the true lower bound of the fragments that can
/// possibly overlap a query — even when an early, large fragment fully spans
/// later, smaller ones — so no candidate that could satisfy a read is ever
/// skipped. This is the standard "stabbing query" trick for interval sets.
pub(crate) struct FragmentStore {
    fragments: Vec<(u64, Vec<u8>)>,
    running_max_end: Vec<u64>,
}

impl FragmentStore {
    pub(crate) fn new() -> Self {
        Self { fragments: Vec::new(), running_max_end: Vec::new() }
    }

    /// Build a store from an unordered `(offset, bytes)` list (e.g. the
    /// ranges/fragments arrays marshalled from the host in one shot), sorting
    /// once.
    pub(crate) fn from_pairs(mut fragments: Vec<(u64, Vec<u8>)>) -> Result<Self, MdfError> {
        fragments.sort_by_key(|(start, _)| *start);
        let mut store = Self { fragments, running_max_end: Vec::new() };
        store.rebuild_running_max()?;
        Ok(store)
    }

    /// Insert one fragment, keeping the store sorted by `offset`.
    pub(crate) fn push(&mut self, offset: u64, bytes: Vec<u8>) -> Result<(), MdfError> {
        let idx = self.fragments.partition_point(|(start, _)| *start <= offset);
        self.fragments.insert(idx, (offset, bytes));
        self.rebuild_running_max()
    }

    fn rebuild_running_max(&mut self) -> Result<(), MdfError> {
        self.running_max_end.clear();
        self.running_max_end.reserve(self.fragments.len());
        let mut max_end = 0u64;
        for (start, bytes) in &self.fragments {
            let end = fragment_end(*start, bytes.len())?;
            max_end = max_end.max(end);
            self.running_max_end.push(max_end);
        }
        Ok(())
    }

    /// The minimal contiguous slice of `fragments` that could possibly
    /// overlap `[offset, req_end)`, found via two binary searches instead of
    /// a linear scan over every stored fragment.
    fn candidates(&self, offset: u64, req_end: u64) -> &[(u64, Vec<u8>)] {
        // Fragments beyond `hi` start at/after `req_end`, so they cannot
        // overlap a half-open request ending at `req_end`.
        let hi = self.fragments.partition_point(|(start, _)| *start < req_end);
        if hi == 0 {
            return &[];
        }
        // `running_max_end` is non-decreasing, so the first index whose
        // running max exceeds `offset` is the earliest fragment that could
        // possibly reach into the request — every fragment before it has
        // `end <= offset` and so cannot overlap.
        let lo = self.running_max_end[..hi].partition_point(|&end| end <= offset);
        &self.fragments[lo..hi]
    }

    /// Try to assemble the absolute range `[offset, offset + length)`.
    /// Returns [`Coverage::Full`] with the bytes, or [`Coverage::Gap`] naming
    /// the first uncovered sub-range — identical semantics to the original
    /// (pre-binary-search) linear-scan implementation.
    pub(crate) fn assemble(&self, offset: u64, length: u64) -> Result<Coverage, MdfError> {
        if length == 0 {
            return Ok(Coverage::Full(Vec::new()));
        }
        let req_end = offset.checked_add(length).ok_or_else(|| {
            MdfError::BlockSerializationError(
                "requested byte range offset + length overflows u64".to_string(),
            )
        })?;
        let len = u64_to_usize(length)?;
        let window = self.candidates(offset, req_end);

        // Fast path: a single fragment fully contains the request.
        for (start, bytes) in window {
            let fstart = *start;
            let fend = fragment_end(fstart, bytes.len())?;
            if offset >= fstart && req_end <= fend {
                let s = u64_to_usize(offset - fstart)?;
                return Ok(Coverage::Full(bytes[s..s + len].to_vec()));
            }
        }

        // Assembly path: stitch the request together from overlapping fragments.
        let mut out = vec![0u8; len];
        let mut covered = vec![false; len];
        for (start, bytes) in window {
            let fstart = *start;
            let fend = fragment_end(fstart, bytes.len())?;
            let lo = offset.max(fstart);
            let hi = req_end.min(fend);
            if lo < hi {
                let dst_lo = u64_to_usize(lo - offset)?;
                let dst_hi = u64_to_usize(hi - offset)?;
                let src_lo = u64_to_usize(lo - fstart)?;
                out[dst_lo..dst_hi].copy_from_slice(&bytes[src_lo..src_lo + (dst_hi - dst_lo)]);
                for c in &mut covered[dst_lo..dst_hi] {
                    *c = true;
                }
            }
        }
        match covered.iter().position(|&c| !c) {
            None => Ok(Coverage::Full(out)),
            Some(pos) => {
                let miss_off = offset + pos as u64;
                Ok(Coverage::Gap { offset: miss_off, length: req_end - miss_off })
            }
        }
    }
}

/// A [`ByteRangeReader`] that serves absolute `(offset, length)` requests from a
/// caller-provided list of file fragments, each tagged with its absolute file
/// offset. A request is satisfied if the union of fragments fully covers it
/// (assembled across fragments when necessary); otherwise a clear error is
/// returned naming the first uncovered offset.
pub(crate) struct FragmentRangeReader {
    pub(crate) store: FragmentStore,
}

impl FragmentRangeReader {
    /// Build a reader from an unordered `(offset, bytes)` fragment list.
    pub(crate) fn from_pairs(fragments: Vec<(u64, Vec<u8>)>) -> Result<Self, MdfError> {
        Ok(Self { store: FragmentStore::from_pairs(fragments)? })
    }
}

impl ByteRangeReader for FragmentRangeReader {
    type Error = MdfError;

    fn read_range(&mut self, offset: u64, length: u64) -> Result<Vec<u8>, MdfError> {
        match self.store.assemble(offset, length)? {
            Coverage::Full(bytes) => Ok(bytes),
            Coverage::Gap { offset: miss, .. } => Err(MdfError::BlockSerializationError(format!(
                "requested byte range {}..{} is not fully covered by the provided fragments \
                 (missing at offset {})",
                offset,
                offset + length,
                miss
            ))),
        }
    }
}

/// A [`ByteRangeReader`] that drives an incremental, range-fetched index
/// build. It serves reads from the fragments gathered so far (borrowed from a
/// [`FragmentStore`] — see there for the lookup complexity) and **gathers**
/// the ranges it still needs in [`misses`](Self::misses) rather than fetching
/// them on demand. [`is_probing`](ByteRangeReader::is_probing) reports `true`:
/// this is precisely the kind of reader the tolerant metadata walk is for — it
/// cannot serve every range, so a failed structural read skips just the
/// unreadable subtree instead of aborting the whole pass.
///
/// The key to avoiding an O(N²) fetch-restart loop is that a single walk does
/// not stop at the first miss:
///
/// - An *optional* leaf read (a channel name/unit/comment text block, via
///   [`read_range_optional`](ByteRangeReader::read_range_optional)) records the
///   gap and returns `Ok(None)`, so the walk carries on and collects every
///   other reachable leaf's gap in the same pass.
/// - A *structural* read (a block header whose bytes yield the next link
///   address, via [`read_range`](ByteRangeReader::read_range)) cannot be
///   satisfied with a placeholder, so it records the gap and returns an error
///   — but because this reader is a probe (`is_probing() == true`), the walk
///   only skips the affected subtree rather than aborting entirely, so every
///   other independent gap is still discovered in the same pass.
///
/// The host driver fetches all recorded ranges (coalesced, with look-ahead)
/// and retries, so a file with N metadata blocks builds in O(passes) ≈ a
/// handful, not O(N).
pub(crate) struct RecordingRangeReader<'a> {
    pub(crate) store: &'a FragmentStore,
    pub(crate) misses: Vec<(u64, u64)>,
}

impl<'a> RecordingRangeReader<'a> {
    pub(crate) fn new(store: &'a FragmentStore) -> Self {
        Self { store, misses: Vec::new() }
    }
}

impl<'a> ByteRangeReader for RecordingRangeReader<'a> {
    type Error = MdfError;

    fn read_range(&mut self, offset: u64, length: u64) -> Result<Vec<u8>, MdfError> {
        match self.store.assemble(offset, length)? {
            Coverage::Full(bytes) => Ok(bytes),
            Coverage::Gap { offset: miss, length: needed } => {
                self.misses.push((miss, needed));
                Err(MdfError::BlockSerializationError(
                    "range not yet available; fetch and retry".to_string(),
                ))
            }
        }
    }

    fn read_range_optional(
        &mut self,
        offset: u64,
        length: u64,
    ) -> Result<Option<Vec<u8>>, MdfError> {
        match self.store.assemble(offset, length)? {
            Coverage::Full(bytes) => Ok(Some(bytes)),
            Coverage::Gap { offset: miss, length: needed } => {
                self.misses.push((miss, needed));
                Ok(None)
            }
        }
    }

    fn is_probing(&self) -> bool {
        true
    }
}
