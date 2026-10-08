//! Downscaling with Lanczos3. Sizes are requested in *displayed* orientation, so a
//! portrait photo stored sideways (EXIF orientation 5–8) is measured the way the
//! user sees it.

use crate::codec::Pixels;
use crate::error::{FinoError, Result};
use crate::options::{Resize, ResizeMode};
use fast_image_resize::{
    images::{Image, ImageRef},
    FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer,
};

/// Orientations 5–8 swap width and height on display.
fn is_transposed(orientation: u16) -> bool {
    (5..=8).contains(&orientation)
}

/// Target stored dimensions, or `None` when the image already fits (never upscales).
pub fn target_size(
    width: u32,
    height: u32,
    orientation: u16,
    resize: Resize,
) -> Option<(u32, u32)> {
    let (shown_w, shown_h) = if is_transposed(orientation) {
        (height, width)
    } else {
        (width, height)
    };
    let scale = match resize.mode {
        ResizeMode::LongEdge => resize.pixels as f64 / shown_w.max(shown_h) as f64,
        ResizeMode::MaxWidth => resize.pixels as f64 / shown_w as f64,
        ResizeMode::MaxHeight => resize.pixels as f64 / shown_h as f64,
    };
    if scale >= 1.0 || resize.pixels == 0 {
        return None;
    }
    let w = ((width as f64 * scale).round() as u32).max(1);
    let h = ((height as f64 * scale).round() as u32).max(1);
    Some((w, h))
}

/// Applies EXIF orientation (1–8) to pixels, so a preview without EXIF displays upright.
pub fn orient(pixels: &Pixels, orientation: u16) -> Pixels {
    if !(2..=8).contains(&orientation) {
        return pixels.clone();
    }
    let (w, h, c) = (
        pixels.width as usize,
        pixels.height as usize,
        pixels.channels as usize,
    );
    let transposed = is_transposed(orientation);
    let (out_w, out_h) = if transposed { (h, w) } else { (w, h) };
    let mut data = vec![0u8; pixels.data.len()];
    for y in 0..out_h {
        for x in 0..out_w {
            // Source coordinates for each destination pixel (EXIF orientation table).
            let (sx, sy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x), // 8
            };
            let (src, dst) = ((sy * w + sx) * c, (y * out_w + x) * c);
            data[dst..dst + c].copy_from_slice(&pixels.data[src..src + c]);
        }
    }
    Pixels {
        data,
        width: out_w as u32,
        height: out_h as u32,
        channels: pixels.channels,
    }
}

/// Output-quality downscale (Lanczos3).
pub fn resize(pixels: &Pixels, width: u32, height: u32) -> Result<Pixels> {
    resize_with(pixels, width, height, FilterType::Lanczos3)
}

/// Thumbnail downscale: an area average, several times cheaper than Lanczos3 at the large
/// factors a UI preview needs, with little aliasing.
pub fn thumbnail(pixels: &Pixels, width: u32, height: u32) -> Result<Pixels> {
    resize_with(pixels, width, height, FilterType::Box)
}

fn resize_with(pixels: &Pixels, width: u32, height: u32, filter: FilterType) -> Result<Pixels> {
    let kind = if pixels.channels == 1 {
        PixelType::U8
    } else {
        PixelType::U8x3
    };
    let src = ImageRef::new(pixels.width, pixels.height, &pixels.data, kind)
        .map_err(|e| FinoError::Resize(e.to_string()))?;
    let mut dst = Image::new(width, height, kind);
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(filter));
    Resizer::new()
        .resize(&src, &mut dst, &options)
        .map_err(|e| FinoError::Resize(e.to_string()))?;
    Ok(Pixels {
        data: dst.into_vec(),
        width,
        height,
        channels: pixels.channels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG_2048: Resize = Resize {
        mode: ResizeMode::LongEdge,
        pixels: 2048,
    };

    #[test]
    fn long_edge_keeps_aspect_ratio() {
        assert_eq!(target_size(6000, 4000, 1, LONG_2048), Some((2048, 1365)));
    }

    #[test]
    fn never_upscales() {
        assert_eq!(target_size(1600, 1200, 1, LONG_2048), None);
    }

    #[test]
    fn max_width_respects_display_orientation() {
        // Stored 6000×4000 but rotated 90° (orientation 6) → shown 4000 wide.
        let r = Resize {
            mode: ResizeMode::MaxWidth,
            pixels: 1000,
        };
        assert_eq!(target_size(6000, 4000, 6, r), Some((1500, 1000)));
        assert_eq!(target_size(6000, 4000, 1, r), Some((1000, 667)));
    }

    #[test]
    fn orientation_6_rotates_clockwise() {
        // 2×1 image [A, B] stored sideways; orientation 6 displays it as a 1×2 column A over B.
        let px = Pixels {
            data: vec![1, 1, 1, 2, 2, 2],
            width: 2,
            height: 1,
            channels: 3,
        };
        let upright = orient(&px, 6);
        assert_eq!((upright.width, upright.height), (1, 2));
        assert_eq!(upright.data, vec![1, 1, 1, 2, 2, 2]);
        let flipped = orient(&px, 3);
        assert_eq!(flipped.data, vec![2, 2, 2, 1, 1, 1]);
        assert_eq!(orient(&px, 1), px);
    }

    #[test]
    fn resizes_pixels_to_requested_dimensions() {
        let src = Pixels {
            data: vec![128; 64 * 32 * 3],
            width: 64,
            height: 32,
            channels: 3,
        };
        let out = resize(&src, 16, 8).unwrap();
        assert_eq!((out.width, out.height, out.data.len()), (16, 8, 16 * 8 * 3));
        assert!(out.data.iter().all(|&v| (126..=130).contains(&v)));
    }
}
