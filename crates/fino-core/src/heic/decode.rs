//! Pixel decoding through macOS ImageIO — the system's licensed, hardware-accelerated HEVC
//! decoder. Also writes the "carrier" JPEG that holds auxiliary images (HDR gain map, depth,
//! portrait matte) so they can be re-attached to Fino's own encode (see `jpeg::mpf`).
//!
//! `CGImageSource` is not thread-safe: each call creates and drops its own.

use crate::codec::Pixels;
use crate::error::{FinoError, Result};
use objc2_core_foundation::{
    CFData, CFDictionary, CFMutableData, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect,
    CGSize,
};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext,
    CGImage, CGImageAlphaInfo,
};
use objc2_image_io::{
    kCGImageAuxiliaryDataTypeDepth, kCGImageAuxiliaryDataTypeDisparity,
    kCGImageAuxiliaryDataTypeHDRGainMap, kCGImageAuxiliaryDataTypePortraitEffectsMatte,
    kCGImagePropertyOrientation, CGImageDestination, CGImageSource,
};
use std::ffi::CStr;
use std::sync::{Condvar, Mutex};

/// The hardware decoder saturates at about two concurrent images; more only costs memory.
const MAX_CONCURRENT_DECODES: usize = 2;
const CARRIER_SIDE: usize = 16;

/// What the decoder hands back for the encoder and the metadata writer.
pub struct Decoded {
    /// Stored pixels, *not* rotated: the orientation travels in EXIF, as in the HEIC.
    pub pixels: Pixels,
    /// Orientation ImageIO derives from `irot`/`imir` (1–8).
    pub orientation: u16,
    /// ICC of the colour space the pixels were drawn in.
    pub icc: Option<Vec<u8>>,
    /// Pixels are in the file's own colour space (not converted to sRGB), so the
    /// container's original ICC bytes describe them too.
    pub native_space: bool,
    /// A small JPEG whose MPF secondaries are the auxiliary images to re-attach.
    pub carrier: Option<Vec<u8>>,
    /// The file has an ISO gain map but this macOS cannot read it (macOS < 15).
    pub unreadable_iso_gain_map: bool,
}

struct Gate {
    busy: Mutex<usize>,
    freed: Condvar,
}

static GATE: Gate = Gate {
    busy: Mutex::new(0),
    freed: Condvar::new(),
};

struct Permit;

impl Permit {
    fn acquire() -> Self {
        let mut busy = GATE.busy.lock().unwrap_or_else(|e| e.into_inner());
        while *busy >= MAX_CONCURRENT_DECODES {
            busy = GATE.freed.wait(busy).unwrap_or_else(|e| e.into_inner());
        }
        *busy += 1;
        Permit
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut busy = GATE.busy.lock().unwrap_or_else(|e| e.into_inner());
        *busy -= 1;
        GATE.freed.notify_one();
    }
}

fn decode_error(what: &str) -> FinoError {
    FinoError::Decode(format!("HEIC: {what}"))
}

/// Keys newer than Fino's minimum macOS (12) are looked up at run time: linking them
/// directly would stop the app from launching on older systems.
fn runtime_key(name: &CStr) -> Option<&'static CFString> {
    // SAFETY: dlsym with RTLD_DEFAULT only looks up a symbol; the ImageIO constants are
    // `CFStringRef` statics that live as long as the process.
    unsafe {
        let symbol = libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) as *const *const CFString;
        if symbol.is_null() || (*symbol).is_null() {
            None
        } else {
            Some(&**symbol)
        }
    }
}

fn number(dict: &CFDictionary, key: &CFString) -> Option<i64> {
    // SAFETY: ImageIO property dictionaries have CFString keys; values are checked by
    // downcast before use.
    let dict: &CFDictionary<CFString, CFType> = unsafe { dict.cast_unchecked() };
    dict.get(key)?.downcast_ref::<CFNumber>()?.as_i64()
}

/// Auxiliary image kinds worth carrying over, by ImageIO key.
fn aux_kinds() -> Vec<&'static CFString> {
    // SAFETY: these ImageIO constants exist since macOS 10.13–11, below Fino's minimum.
    let mut kinds: Vec<&'static CFString> = unsafe {
        vec![
            kCGImageAuxiliaryDataTypeHDRGainMap,
            kCGImageAuxiliaryDataTypeDepth,
            kCGImageAuxiliaryDataTypeDisparity,
            kCGImageAuxiliaryDataTypePortraitEffectsMatte,
        ]
    };
    kinds.extend(runtime_key(c"kCGImageAuxiliaryDataTypeISOGainMap"));
    kinds
}

fn draw_rgb(image: &CGImage) -> Result<(Pixels, Option<Vec<u8>>, bool)> {
    let (w, h) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    let rgb_space = CGImage::color_space(Some(image))
        .filter(|cs| CGColorSpace::number_of_components(Some(cs)) == 3 && !cs.uses_itur_2100_tf());
    let native_space = rgb_space.is_some();
    // SAFETY: kCGColorSpaceSRGB is a framework constant.
    let space = match rgb_space {
        Some(cs) => cs,
        None => CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
            .ok_or_else(|| decode_error("no colour space"))?,
    };
    let icc = CGColorSpace::icc_data(Some(&space)).map(|d| d.to_vec());
    let stride = w * 4;
    let mut rgbx = vec![0u8; stride * h];
    {
        // SAFETY: `rgbx` holds exactly `stride * h` bytes, outlives the context (dropped at
        // the end of this block) and matches the 8-bit RGBX layout requested.
        let ctx: CFRetained<CGContext> = unsafe {
            CGBitmapContextCreate(
                rgbx.as_mut_ptr().cast(),
                w,
                h,
                8,
                stride,
                Some(&space),
                CGImageAlphaInfo::NoneSkipLast.0,
            )
        }
        .ok_or_else(|| decode_error("bitmap context"))?;
        let rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: w as f64,
                height: h as f64,
            },
        };
        CGContext::draw_image(Some(&ctx), rect, Some(image)); // HEVC decoding happens here
    }
    let mut rgb = Vec::with_capacity(w * h * 3);
    for px in rgbx.as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
    }
    let pixels = Pixels {
        data: rgb,
        width: w as u32,
        height: h as u32,
        channels: 3,
    };
    Ok((pixels, icc, native_space))
}

/// A tiny JPEG carrying `aux` auxiliary images, written by ImageIO in its MPF layout.
fn carrier(aux: &[(&CFString, CFRetained<CFDictionary>)]) -> Option<Vec<u8>> {
    let mut pixels = vec![128u8; CARRIER_SIDE * CARRIER_SIDE * 4];
    // SAFETY: as in `draw_rgb`; the buffer outlives the context and image creation.
    let dummy = unsafe {
        let space = CGColorSpace::with_name(Some(kCGColorSpaceSRGB))?;
        let ctx = CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            CARRIER_SIDE,
            CARRIER_SIDE,
            8,
            CARRIER_SIDE * 4,
            Some(&space),
            CGImageAlphaInfo::NoneSkipLast.0,
        )?;
        CGBitmapContextCreateImage(Some(&ctx))?
    };
    let out = CFMutableData::new(None, 0)?;
    let jpeg = CFString::from_static_str("public.jpeg");
    // SAFETY: the destination writes into `out`; the dictionaries came from ImageIO itself.
    unsafe {
        let dest = CGImageDestination::with_data(&out, &jpeg, 1, None)?;
        dest.add_image(&dummy, None);
        for (kind, info) in aux {
            dest.add_auxiliary_data_info(kind, info);
        }
        if !dest.finalize() {
            return None;
        }
    }
    Some(out.to_vec())
}

/// Decodes the primary image of a HEIC file. `want_aux` asks for the carrier JPEG.
pub fn decode(bytes: &[u8], want_aux: bool) -> Result<Decoded> {
    let _permit = Permit::acquire();
    let data = CFData::from_bytes(bytes);
    // SAFETY: plain ImageIO calls on objects created and dropped within this function.
    let source = unsafe { CGImageSource::with_data(&data, None) }
        .ok_or_else(|| decode_error("not an image"))?;
    let index = unsafe { source.primary_image_index() };
    let properties = unsafe { source.properties_at_index(index, None) }
        .ok_or_else(|| decode_error("no image properties"))?;
    let orientation = number(&properties, unsafe { kCGImagePropertyOrientation })
        .filter(|o| (1..=8).contains(o))
        .unwrap_or(1) as u16;

    let aux: Vec<(&CFString, CFRetained<CFDictionary>)> = if want_aux {
        aux_kinds()
            .into_iter()
            .filter_map(|kind| {
                let info = unsafe { source.auxiliary_data_info_at_index(index, kind) }?;
                Some((kind, info))
            })
            .collect()
    } else {
        vec![]
    };
    let carrier = if aux.is_empty() {
        None
    } else {
        Some(carrier(&aux).ok_or_else(|| decode_error("could not copy the HDR gain map"))?)
    };
    let image = unsafe { source.image_at_index(index, None) }
        .ok_or_else(|| decode_error("decoding failed"))?;
    let (pixels, icc, native_space) = draw_rgb(&image)?;
    Ok(Decoded {
        pixels,
        orientation,
        icc,
        native_space,
        carrier,
        unreadable_iso_gain_map: runtime_key(c"kCGImageAuxiliaryDataTypeISOGainMap").is_none(),
    })
}

/// Auxiliary image kinds ImageIO finds in any image file (used to verify outputs).
pub fn aux_present(bytes: &[u8]) -> Vec<String> {
    let data = CFData::from_bytes(bytes);
    // SAFETY: as in `decode`.
    let Some(source) = (unsafe { CGImageSource::with_data(&data, None) }) else {
        return vec![];
    };
    let index = unsafe { source.primary_image_index() };
    aux_kinds()
        .into_iter()
        .filter(|kind| unsafe { source.auxiliary_data_info_at_index(index, kind) }.is_some())
        .map(|kind| kind.to_string())
        .collect()
}
