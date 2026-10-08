//! Lossless recompression in the DCT domain (what `jpegtran -optimize -progressive` does).
//!
//! The quantized coefficients are copied untouched; only the entropy coding changes —
//! optimal Huffman tables and a progressive scan script. Decoded pixels are bit-identical,
//! yet many camera, editor and optimizer files (standard Huffman tables, baseline) shrink
//! by a few percent.

use crate::error::{FinoError, Result};
use mozjpeg_sys as ffi;
use std::os::raw::{c_int, c_ulong};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[cold]
unsafe extern "C-unwind" fn unwind_on_error(cinfo: &mut ffi::jpeg_common_struct) {
    let code = cinfo.err.as_ref().map(|e| e.msg_code).unwrap_or(-1);
    std::panic::resume_unwind(Box::new(format!("libjpeg error {code}")));
}

unsafe extern "C-unwind" fn silence(_cinfo: &mut ffi::jpeg_common_struct, _level: c_int) {}

pub(crate) fn error_mgr() -> Box<ffi::jpeg_error_mgr> {
    // SAFETY: jpeg_error_mgr is a plain C struct; jpeg_std_error fully initializes it
    // before we override the two callbacks.
    unsafe {
        let mut err: Box<ffi::jpeg_error_mgr> = Box::new(std::mem::zeroed());
        ffi::jpeg_std_error(&mut err);
        err.error_exit = Some(unwind_on_error);
        err.emit_message = Some(silence);
        err
    }
}

/// Owns both libjpeg objects so they are destroyed even when libjpeg unwinds mid-way.
struct Transcoder {
    src: Box<ffi::jpeg_decompress_struct>,
    dst: Box<ffi::jpeg_compress_struct>,
    _src_err: Box<ffi::jpeg_error_mgr>,
    _dst_err: Box<ffi::jpeg_error_mgr>,
    out_buf: *mut u8,
    out_len: c_ulong,
    /// libjpeg only publishes its (re)allocated output buffer to `out_buf` when compression
    /// finishes; before that `out_buf` may point at memory libjpeg already freed while
    /// growing. Freeing it then would be a double free, so it is only freed once finished.
    finished: bool,
}

impl Transcoder {
    fn new() -> Self {
        let mut src_err = error_mgr();
        let mut dst_err = error_mgr();
        // SAFETY: zeroed structs are the documented starting state before jpeg_create_*;
        // `err` points at boxed managers that outlive the structs (same owner).
        unsafe {
            let mut src: Box<ffi::jpeg_decompress_struct> = Box::new(std::mem::zeroed());
            let mut dst: Box<ffi::jpeg_compress_struct> = Box::new(std::mem::zeroed());
            src.common.err = &mut *src_err;
            dst.common.err = &mut *dst_err;
            ffi::jpeg_create_decompress(&mut *src);
            ffi::jpeg_create_compress(&mut *dst);
            Self {
                src,
                dst,
                _src_err: src_err,
                _dst_err: dst_err,
                out_buf: std::ptr::null_mut(),
                out_len: 0,
                finished: false,
            }
        }
    }

    /// SAFETY: `data` must stay alive until this call returns (libjpeg reads it in place).
    unsafe fn run(&mut self, data: &[u8]) -> Vec<u8> {
        ffi::jpeg_mem_src(&mut self.src, data.as_ptr(), data.len() as c_ulong);
        ffi::jpeg_read_header(&mut self.src, 1);
        let coefficients = ffi::jpeg_read_coefficients(&mut self.src);

        ffi::jpeg_copy_critical_parameters(&self.src, &mut self.dst);
        ffi::jpeg_mem_dest(&mut self.dst, &mut self.out_buf, &mut self.out_len);
        // A plain progressive script: optimize_scans would try many scripts for ~0.5 % more.
        ffi::jpeg_c_set_bool_param(
            &mut self.dst,
            ffi::J_BOOLEAN_PARAM::JBOOLEAN_OPTIMIZE_SCANS,
            0,
        );
        self.dst.optimize_coding = 1;
        ffi::jpeg_simple_progression(&mut self.dst);
        ffi::jpeg_write_coefficients(&mut self.dst, coefficients);
        ffi::jpeg_finish_compress(&mut self.dst);
        self.finished = true;
        ffi::jpeg_finish_decompress(&mut self.src);

        std::slice::from_raw_parts(self.out_buf, self.out_len as usize).to_vec()
    }
}

impl Drop for Transcoder {
    fn drop(&mut self) {
        // SAFETY: both structs were created in `new`; destroy is valid in any state, and
        // the memory destination buffer comes from malloc inside jpeg_mem_dest.
        unsafe {
            ffi::jpeg_destroy_compress(&mut self.dst);
            ffi::jpeg_destroy_decompress(&mut self.src);
            // On an error mid-write the in-flight buffer leaks (bounded by the output size)
            // rather than risking a double free.
            if self.finished && !self.out_buf.is_null() {
                libc::free(self.out_buf.cast());
            }
        }
    }
}

/// Re-encodes `data` without touching a single coefficient. The output carries no APPn/COM
/// metadata — callers splice the original segments back in.
pub fn transcode(data: &[u8]) -> Result<Vec<u8>> {
    catch_unwind(AssertUnwindSafe(|| {
        let mut transcoder = Transcoder::new();
        // SAFETY: `data` outlives the call; all libjpeg state is owned by `transcoder`.
        unsafe { transcoder.run(data) }
    }))
    .map_err(|panic| {
        let msg = panic
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "libjpeg error".into());
        FinoError::Encode(msg)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{self, Pixels};

    /// Baseline JPEG with the *standard* Huffman tables, like cameras write.
    fn standard_huffman_jpeg() -> Vec<u8> {
        let (w, h) = (256u32, 192u32);
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let n = ((x * 7 + y * 13) ^ (x * y)) % 31;
                data.extend_from_slice(&[(x + n) as u8, (y + n) as u8, ((x + y) / 2 + n) as u8]);
            }
        }
        let px = Pixels {
            data,
            width: w,
            height: h,
            channels: 3,
        };
        let mut c = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
        c.set_fastest_defaults();
        c.set_size(w as usize, h as usize);
        c.set_quality(85.0);
        c.set_optimize_coding(false);
        let mut s = c.start_compress(Vec::new()).unwrap();
        s.write_scanlines(&px.data).unwrap();
        s.finish().unwrap()
    }

    #[test]
    fn shrinks_standard_huffman_files_with_identical_pixels() {
        let original = standard_huffman_jpeg();
        let out = transcode(&original).unwrap();
        assert!(
            out.len() < original.len(),
            "{} !< {}",
            out.len(),
            original.len()
        );
        let a = codec::decode(&original, false).unwrap();
        let b = codec::decode(&out, false).unwrap();
        assert_eq!(a.data, b.data, "pixels must be bit-identical");
        assert_eq!(
            crate::jpeg::inspect(&out).unwrap().kind,
            crate::jpeg::FrameKind::Progressive
        );
    }

    #[test]
    fn garbage_is_an_error_not_a_crash() {
        assert!(transcode(b"\xFF\xD8\xFF\xE0 definitely not a jpeg").is_err());
        let mut truncated = standard_huffman_jpeg();
        truncated.truncate(truncated.len() / 2);
        let _ = transcode(&truncated); // may succeed with a warning or fail — must not crash
    }
}
