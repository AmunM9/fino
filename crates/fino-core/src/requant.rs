//! Re-quantization in the DCT domain: a JPEG's own coefficients divided down to coarser
//! tables, then entropy-coded again.
//!
//! Compared with decoding to pixels and encoding again, nothing is converted, transformed
//! or resampled: a candidate differs from the source only by the new quantization (no
//! second round of colour conversion, DCT rounding or chroma resampling), and each probe
//! skips the most expensive half of an encode.

use crate::error::{FinoError, Result};
use crate::lossless::error_mgr;
use mozjpeg_sys as ffi;
use rayon::prelude::*;
use std::os::raw::{c_int, c_ulong};
use std::panic::{catch_unwind, AssertUnwindSafe};

const BLOCK: usize = 64;

/// How a candidate is entropy-coded. Both decode to the same pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coding {
    /// Baseline with the standard Huffman tables: the cheapest to write, for search probes.
    Probe,
    /// Progressive (spectral selection only) with optimal Huffman tables: the output.
    Final,
}

/// One component's blocks: row pointers into a libjpeg virtual array kept fully in memory.
struct Plane {
    rows: Vec<ffi::JBLOCKROW>,
    blocks_per_row: usize,
}

/// Raw row pointers shared with worker threads; every row is written by exactly one.
#[derive(Clone, Copy)]
struct Rows(*const ffi::JBLOCKROW);
// SAFETY: the pointed-to rows are disjoint per index and outlive the parallel loop.
unsafe impl Send for Rows {}
unsafe impl Sync for Rows {}

/// A source JPEG's quantized coefficients, read once and re-quantized for every candidate.
pub struct Coefficients {
    src: Box<ffi::jpeg_decompress_struct>,
    _err: Box<ffi::jpeg_error_mgr>,
    source: Vec<Plane>,
    workspace: Vec<Plane>,
    workspace_arrays: Vec<*mut ffi::jvirt_barray_control>,
    /// Per component: the source's quantization steps (natural order) and its table slot.
    source_steps: Vec<[u16; BLOCK]>,
    slots: Vec<usize>,
}

// SAFETY: the libjpeg objects are not tied to the thread that created them; `Coefficients`
// is only ever used by one thread at a time (callers hold it behind `&mut` or a lock).
unsafe impl Send for Coefficients {}

fn access(
    cinfo: &mut ffi::jpeg_common_struct,
    array: *mut ffi::jvirt_barray_control,
    rows: usize,
    chunk: usize,
    writable: bool,
) -> Vec<ffi::JBLOCKROW> {
    let mut out = Vec::with_capacity(rows);
    let mut start = 0;
    while start < rows {
        let n = chunk.min(rows - start);
        // SAFETY: `array` was realized by this `cinfo`; rows are accessed in order and in
        // chunks no larger than its `maxaccess`. Arrays live entirely in memory (no backing
        // store), so the returned row pointers stay valid until the object is destroyed.
        unsafe {
            let access = (*cinfo.mem)
                .access_virt_barray
                .expect("libjpeg memory manager");
            let block_rows = access(cinfo, array, start as _, n as _, writable as _);
            out.extend((0..n).map(|i| *block_rows.add(i)));
        }
        start += n;
    }
    out
}

impl Coefficients {
    /// Reads the coefficients of a baseline or progressive Huffman/arithmetic JPEG.
    pub fn read(data: &[u8]) -> Result<Self> {
        catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: `data` outlives `read_unguarded`, which consumes it fully.
            unsafe { Self::read_unguarded(data) }
        }))
        .map_err(|_| FinoError::Decode("unreadable coefficients".into()))?
    }

    unsafe fn read_unguarded(data: &[u8]) -> Result<Self> {
        let mut err = error_mgr();
        let mut src: Box<ffi::jpeg_decompress_struct> = Box::new(std::mem::zeroed());
        src.common.err = &mut *err;
        ffi::jpeg_create_decompress(&mut *src);
        let mut this = Self {
            src,
            _err: err,
            source: Vec::new(),
            workspace: Vec::new(),
            workspace_arrays: Vec::new(),
            source_steps: Vec::new(),
            slots: Vec::new(),
        };
        let src = &mut *this.src;
        ffi::jpeg_mem_src(src, data.as_ptr(), data.len() as c_ulong);
        ffi::jpeg_read_header(src, 1);
        // Only YCbCr and grayscale: an RGB-coded file would be written with a colour
        // transform marker that metadata splicing drops.
        if !matches!(
            src.jpeg_color_space,
            ffi::J_COLOR_SPACE::JCS_YCbCr | ffi::J_COLOR_SPACE::JCS_GRAYSCALE
        ) {
            return Err(FinoError::Decode("not a YCbCr or grayscale JPEG".into()));
        }
        let components = std::slice::from_raw_parts(src.comp_info, src.num_components as usize);
        // Workspace arrays must be requested before jpeg_read_coefficients realizes them.
        let mut geometry = Vec::new();
        for c in components {
            let (h, v) = (c.h_samp_factor as usize, c.v_samp_factor as usize);
            let width = (c.width_in_blocks as usize).div_ceil(h) * h;
            let height = (c.height_in_blocks as usize).div_ceil(v) * v;
            let request = (*src.common.mem)
                .request_virt_barray
                .expect("libjpeg memory manager");
            let array = request(
                &mut src.common,
                ffi::JPOOL_IMAGE,
                1,
                width as _,
                height as _,
                v as _,
            );
            this.workspace_arrays.push(array);
            geometry.push((width, height, v));
        }
        let arrays = ffi::jpeg_read_coefficients(src);
        let components = std::slice::from_raw_parts(src.comp_info, src.num_components as usize);
        for (i, (c, &(width, height, v))) in components.iter().zip(&geometry).enumerate() {
            let slot = c.quant_tbl_no as usize;
            let table = src.quant_tbl_ptrs[slot];
            assert!(!table.is_null(), "component without a quantization table");
            let mut steps = [0u16; BLOCK];
            steps.copy_from_slice(&(*table).quantval);
            this.source_steps.push(steps);
            this.slots.push(slot);
            let blocks_per_row = c.width_in_blocks as usize;
            let rows = c.height_in_blocks as usize;
            this.source.push(Plane {
                rows: access(&mut src.common, *arrays.add(i), rows, v, false),
                blocks_per_row,
            });
            let mut workspace = access(&mut src.common, this.workspace_arrays[i], height, v, true);
            workspace.truncate(rows);
            this.workspace.push(Plane {
                rows: workspace,
                blocks_per_row: blocks_per_row.min(width),
            });
        }
        Ok(this)
    }

    pub fn is_grayscale(&self) -> bool {
        self.source.len() == 1
    }

    /// Fills the workspace with the source re-quantized to `steps` (one table per component).
    fn requantize(&mut self, steps: &[[u16; BLOCK]]) {
        for (c, plane) in self.source.iter().enumerate() {
            let ratio: [f32; BLOCK] =
                std::array::from_fn(|k| self.source_steps[c][k] as f32 / steps[c][k] as f32);
            let (from, to) = (
                Rows(plane.rows.as_ptr()),
                Rows(self.workspace[c].rows.as_ptr()),
            );
            let width = plane.blocks_per_row;
            (0..plane.rows.len()).into_par_iter().for_each(move |y| {
                let (from, to) = (from, to);
                // SAFETY: row `y` exists in both arrays (same geometry) and only this
                // iteration touches it; each row holds `width` blocks.
                let (src, dst) = unsafe {
                    (
                        std::slice::from_raw_parts(*from.0.add(y), width),
                        std::slice::from_raw_parts_mut(*to.0.add(y), width),
                    )
                };
                for (a, b) in src.iter().zip(dst.iter_mut()) {
                    for k in 0..BLOCK {
                        let v = a[k] as f32 * ratio[k];
                        b[k] = (v + 0.5f32.copysign(v)) as i16;
                    }
                }
            });
        }
    }

    /// Encodes the source re-quantized to `luma` / `chroma` tables (natural order). Steps
    /// never go below the source's own: a finer step cannot bring detail back. Components
    /// sharing a table slot in the source share the first one's table.
    pub fn encode(
        &mut self,
        luma: &[u16; BLOCK],
        chroma: &[u16; BLOCK],
        coding: Coding,
    ) -> Result<Vec<u8>> {
        // One table per slot, as the file will carry it: components sharing a slot share the
        // steps of the first one using it.
        let mut by_slot: [Option<[u16; BLOCK]>; 4] = [None; 4];
        for (c, source) in self.source_steps.iter().enumerate() {
            let target = if c == 0 { luma } else { chroma };
            by_slot[self.slots[c]].get_or_insert_with(|| {
                std::array::from_fn(|k| target[k].max(source[k]).clamp(1, 255))
            });
        }
        let steps: Vec<[u16; BLOCK]> = self
            .slots
            .iter()
            .map(|&slot| by_slot[slot].expect("set for every component above"))
            .collect();
        self.requantize(&steps);
        catch_unwind(AssertUnwindSafe(|| {
            let mut writer = Writer::new();
            // SAFETY: the workspace arrays belong to `self.src`, which outlives the call.
            unsafe { writer.run(self, &steps, coding) }
        }))
        .map_err(|panic| {
            let msg = panic
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "libjpeg error".into());
            FinoError::Encode(msg)
        })
    }
}

impl Drop for Coefficients {
    fn drop(&mut self) {
        // SAFETY: created in `read_unguarded`; destroy is valid in any state.
        unsafe { ffi::jpeg_destroy_decompress(&mut self.src) }
    }
}

/// Progressive script without successive approximation: refinement scans cost a lot of
/// encode time for a fraction of a percent.
fn spectral_scans(components: usize) -> Vec<ffi::jpeg_scan_info> {
    let scan = |comps: &[c_int], ss: c_int, se: c_int| {
        let mut component_index = [0; 4];
        component_index[..comps.len()].copy_from_slice(comps);
        ffi::jpeg_scan_info {
            comps_in_scan: comps.len() as c_int,
            component_index,
            Ss: ss,
            Se: se,
            Ah: 0,
            Al: 0,
        }
    };
    if components == 3 {
        vec![
            scan(&[0, 1, 2], 0, 0),
            scan(&[0], 1, 5),
            scan(&[1], 1, 63),
            scan(&[2], 1, 63),
            scan(&[0], 6, 63),
        ]
    } else {
        let all: Vec<c_int> = (0..components as c_int).collect();
        let mut scans = vec![scan(&all, 0, 0)];
        scans.extend((0..components as c_int).map(|c| scan(&[c], 1, 63)));
        scans
    }
}

/// Owns the compressor so it is destroyed even when libjpeg unwinds mid-way.
struct Writer {
    dst: Box<ffi::jpeg_compress_struct>,
    _err: Box<ffi::jpeg_error_mgr>,
    scans: Vec<ffi::jpeg_scan_info>,
    out_buf: *mut u8,
    out_len: c_ulong,
    /// See `lossless::Transcoder::finished`: the buffer is only ours once compression ends.
    finished: bool,
}

impl Writer {
    fn new() -> Self {
        let mut err = error_mgr();
        // SAFETY: zeroed struct is the documented state before jpeg_create_compress; `err`
        // is boxed and owned alongside it.
        unsafe {
            let mut dst: Box<ffi::jpeg_compress_struct> = Box::new(std::mem::zeroed());
            dst.common.err = &mut *err;
            ffi::jpeg_create_compress(&mut *dst);
            Self {
                dst,
                _err: err,
                scans: Vec::new(),
                out_buf: std::ptr::null_mut(),
                out_len: 0,
                finished: false,
            }
        }
    }

    unsafe fn run(
        &mut self,
        coefficients: &mut Coefficients,
        steps: &[[u16; BLOCK]],
        coding: Coding,
    ) -> Vec<u8> {
        let dst = &mut *self.dst;
        // Plain libjpeg defaults (no mozjpeg scan search): set before the defaults are applied.
        ffi::jpeg_c_set_int_param(
            dst,
            ffi::J_INT_PARAM::JINT_COMPRESS_PROFILE,
            ffi::JINT_COMPRESS_PROFILE_VALUE::JCP_FASTEST as c_int,
        );
        ffi::jpeg_copy_critical_parameters(&coefficients.src, dst);
        for (c, table) in steps.iter().enumerate() {
            let basic: [u32; BLOCK] = std::array::from_fn(|k| table[k] as u32);
            ffi::jpeg_add_quant_table(dst, coefficients.slots[c] as c_int, basic.as_ptr(), 100, 1);
        }
        ffi::jpeg_mem_dest(dst, &mut self.out_buf, &mut self.out_len);
        if coding == Coding::Final {
            dst.optimize_coding = 1;
            self.scans = spectral_scans(coefficients.source.len());
            dst.scan_info = self.scans.as_ptr();
            dst.num_scans = self.scans.len() as c_int;
        }
        ffi::jpeg_write_coefficients(dst, coefficients.workspace_arrays.as_mut_ptr());
        ffi::jpeg_finish_compress(dst);
        self.finished = true;
        std::slice::from_raw_parts(self.out_buf, self.out_len as usize).to_vec()
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // SAFETY: created in `new`; the memory destination buffer comes from malloc inside
        // jpeg_mem_dest and is only freed once libjpeg handed it over.
        unsafe {
            ffi::jpeg_destroy_compress(&mut self.dst);
            if self.finished && !self.out_buf.is_null() {
                libc::free(self.out_buf.cast());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{self, Effort, EncodeParams, Pixels};

    fn source(chroma: (u8, u8)) -> Vec<u8> {
        let (w, h) = (200u32, 136u32);
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let n = ((x * 7 + y * 13) ^ (x * y)) % 29;
                data.extend_from_slice(&[(x + n) as u8, (y + n) as u8, ((x + y) / 2 + n) as u8]);
            }
        }
        let px = Pixels {
            data,
            width: w,
            height: h,
            channels: 3,
        };
        codec::encode(&px, EncodeParams::new(95.0, chroma, Effort::Probe)).unwrap()
    }

    fn table(quality: f32) -> [u16; BLOCK] {
        let t = mozjpeg::qtable::NRobidoux.scaled(quality, quality);
        // SAFETY: a QTable holds 64 entries.
        let values = unsafe { std::slice::from_raw_parts(t.as_ptr(), BLOCK) };
        std::array::from_fn(|k| values[k] as u16)
    }

    #[test]
    fn probe_and_final_codings_decode_to_identical_pixels() {
        for chroma in [(1, 1), (2, 2), (2, 1)] {
            let original = source(chroma);
            let mut coefficients = Coefficients::read(&original).unwrap();
            let t = table(70.0);
            let probe = coefficients.encode(&t, &t, Coding::Probe).unwrap();
            let fin = coefficients.encode(&t, &t, Coding::Final).unwrap();
            assert!(fin.len() < probe.len(), "{chroma:?}");
            assert!(probe.len() < original.len(), "{chroma:?}");
            let a = codec::decode(&probe, false).unwrap();
            let b = codec::decode(&fin, false).unwrap();
            assert_eq!(a.data, b.data, "{chroma:?}");
            assert_eq!((a.width, a.height), (200, 136));
            assert_eq!(
                crate::jpeg::inspect(&fin).unwrap().kind,
                crate::jpeg::FrameKind::Progressive
            );
            assert_eq!(crate::jpeg::inspect(&fin).unwrap().chroma_block(), chroma);
        }
    }

    #[test]
    fn coarser_tables_give_smaller_files_and_the_source_table_is_a_floor() {
        let original = source((2, 2));
        let mut coefficients = Coefficients::read(&original).unwrap();
        let fine = coefficients
            .encode(&[1; BLOCK], &[1; BLOCK], Coding::Final)
            .unwrap();
        let (hi, lo) = (table(85.0), table(60.0));
        let high = coefficients.encode(&hi, &hi, Coding::Final).unwrap();
        let low = coefficients.encode(&lo, &lo, Coding::Final).unwrap();
        assert!(low.len() < high.len() && high.len() < fine.len());
        // Asking for steps finer than the source keeps its coefficients exactly.
        let a = codec::decode(&original, false).unwrap();
        let b = codec::decode(&fine, false).unwrap();
        assert_eq!(a.data, b.data);
    }

    #[test]
    fn odd_sizes_and_grayscale_round_trip() {
        let (w, h) = (203u32, 131u32);
        let gray = Pixels {
            data: (0..w * h).map(|i| ((i * 7) % 251) as u8).collect(),
            width: w,
            height: h,
            channels: 1,
        };
        let rgb = Pixels {
            data: (0..w * h * 3).map(|i| ((i * 13) % 241) as u8).collect(),
            width: w,
            height: h,
            channels: 3,
        };
        for (px, chroma) in [(&gray, (1, 1)), (&rgb, (2, 2)), (&rgb, (2, 1))] {
            let original =
                codec::encode(px, EncodeParams::new(92.0, chroma, Effort::Probe)).unwrap();
            let mut coefficients = Coefficients::read(&original).unwrap();
            assert_eq!(coefficients.is_grayscale(), px.channels == 1);
            let t = table(75.0);
            let probe = coefficients.encode(&t, &t, Coding::Probe).unwrap();
            let fin = coefficients.encode(&t, &t, Coding::Final).unwrap();
            let gray = px.channels == 1;
            let a = codec::decode(&probe, gray).unwrap();
            let b = codec::decode(&fin, gray).unwrap();
            assert_eq!((a.width, a.height), (w, h));
            assert_eq!(a.data, b.data, "{chroma:?}");
        }
    }

    #[test]
    fn garbage_is_an_error_not_a_crash() {
        assert!(Coefficients::read(b"\xFF\xD8\xFF\xE0 not a jpeg").is_err());
        let mut truncated = source((2, 2));
        truncated.truncate(truncated.len() / 3);
        if let Ok(mut c) = Coefficients::read(&truncated) {
            let t = table(70.0);
            let _ = c.encode(&t, &t, Coding::Final);
        }
    }
}
