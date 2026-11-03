use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineMetadata {
    pub width: u32,
    pub height: u32,
    pub os: String,
}

/// Fixed per-pixel anti-aliasing tolerance on the normalized (0..1) YIQ
/// perceptual distance — distinct from the caller-facing, configurable
/// image-level ratio threshold (design.md §7 default 0.001).
pub const PIXEL_SENSITIVITY: f64 = 0.1;

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Debug)]
pub enum DiffError {
    Decode(String),
    DimensionMismatch {
        baseline: (u32, u32),
        current: (u32, u32),
    },
}

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiffError::Decode(e) => write!(f, "failed to decode image: {e}"),
            DiffError::DimensionMismatch { baseline, current } => write!(
                f,
                "baseline is {}x{} but current screenshot is {}x{}",
                baseline.0, baseline.1, current.0, current.1
            ),
        }
    }
}
impl std::error::Error for DiffError {}

pub struct DiffOutcome {
    pub ratio: f64,
    pub differing_pixels: u64,
    pub compared_pixels: u64,
    pub diff_png: Vec<u8>,
}

fn yiq(p: &Rgba<u8>) -> (f64, f64, f64) {
    let r = p.0[0] as f64;
    let g = p.0[1] as f64;
    let b = p.0[2] as f64;
    let y = r * 0.298_895_31 + g * 0.586_622_47 + b * 0.114_482_23;
    let i = r * 0.595_977_99 - g * 0.274_176_10 - b * 0.321_801_89;
    let q = r * 0.211_470_17 - g * 0.522_617_11 + b * 0.311_146_94;
    (y, i, q)
}

/// Normalized 0..1 perceptual distance. The Y/I/Q weights (0.5053, 0.299,
/// 0.1957) sum to 1.0, so the maximum possible weighted distance (every
/// channel 255 apart) is exactly 255 — dividing by 255 gives a clean 0..1
/// scale without a separately-tuned normalization constant.
fn perceptual_distance(a: &Rgba<u8>, b: &Rgba<u8>) -> f64 {
    let (y1, i1, q1) = yiq(a);
    let (y2, i2, q2) = yiq(b);
    let dy = y1 - y2;
    let di = i1 - i2;
    let dq = q1 - q2;
    let weighted = (0.5053 * dy * dy + 0.299 * di * di + 0.1957 * dq * dq).sqrt();
    weighted / 255.0
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, DiffError> {
    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;
    let (w, h) = img.dimensions();
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .map_err(|e| DiffError::Decode(e.to_string()))?;
    Ok(out)
}

/// Compares two PNG-encoded screenshots pixel-by-pixel. `masks` are pixel
/// rectangles excluded from both the differing count and the denominator
/// (design.md §17 — element-based masks are a v1.1 refinement; v1 works in
/// raw coordinates so this stays decoupled from the Chroma element index).
/// Masked pixels are shaded gray in the returned diff image so a reviewer
/// can see what was excluded, not just what was compared.
pub fn diff_images(
    baseline_png: &[u8],
    current_png: &[u8],
    masks: &[Rect],
) -> Result<DiffOutcome, DiffError> {
    let baseline = image::load_from_memory(baseline_png)
        .map_err(|e| DiffError::Decode(e.to_string()))?
        .to_rgba8();
    let current = image::load_from_memory(current_png)
        .map_err(|e| DiffError::Decode(e.to_string()))?
        .to_rgba8();

    let (bw, bh) = baseline.dimensions();
    let (cw, ch) = current.dimensions();
    if (bw, bh) != (cw, ch) {
        return Err(DiffError::DimensionMismatch {
            baseline: (bw, bh),
            current: (cw, ch),
        });
    }

    let mut diff_img: RgbaImage = current.clone();
    let mut differing: u64 = 0;
    let mut compared: u64 = 0;

    for y in 0..bh {
        for x in 0..bw {
            if masks.iter().any(|m| m.contains(x, y)) {
                diff_img.put_pixel(x, y, Rgba([128, 128, 128, 255]));
                continue;
            }
            compared += 1;
            let p1 = baseline.get_pixel(x, y);
            let p2 = current.get_pixel(x, y);
            if perceptual_distance(p1, p2) > PIXEL_SENSITIVITY {
                differing += 1;
                diff_img.put_pixel(x, y, Rgba([255, 0, 255, 255]));
            }
        }
    }

    let ratio = if compared == 0 {
        0.0
    } else {
        differing as f64 / compared as f64
    };
    let diff_png = encode_png(&diff_img)?;

    Ok(DiffOutcome {
        ratio,
        differing_pixels: differing,
        compared_pixels: compared,
        diff_png,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_png(w: u32, h: u32, color: [u8; 4]) -> Vec<u8> {
        let img: RgbaImage = image::ImageBuffer::from_fn(w, h, |_, _| Rgba(color));
        encode_png(&img).unwrap()
    }

    fn png_with_one_pixel(
        w: u32,
        h: u32,
        base: [u8; 4],
        px: (u32, u32),
        color: [u8; 4],
    ) -> Vec<u8> {
        let img: RgbaImage = image::ImageBuffer::from_fn(w, h, |x, y| {
            if (x, y) == px {
                Rgba(color)
            } else {
                Rgba(base)
            }
        });
        encode_png(&img).unwrap()
    }

    #[test]
    fn identical_images_have_zero_ratio() {
        let a = solid_png(10, 10, [10, 20, 30, 255]);
        let b = solid_png(10, 10, [10, 20, 30, 255]);

        let outcome = diff_images(&a, &b, &[]).unwrap();

        assert_eq!(outcome.ratio, 0.0);
        assert_eq!(outcome.differing_pixels, 0);
        assert_eq!(outcome.compared_pixels, 100);
    }

    #[test]
    fn one_pixel_difference_yields_a_small_nonzero_ratio() {
        let base = solid_png(10, 10, [0, 0, 0, 255]);
        let changed = png_with_one_pixel(10, 10, [0, 0, 0, 255], (3, 4), [255, 255, 255, 255]);

        let outcome = diff_images(&base, &changed, &[]).unwrap();

        assert_eq!(outcome.differing_pixels, 1);
        assert_eq!(outcome.compared_pixels, 100);
        assert!((outcome.ratio - 0.01).abs() < 1e-9);
    }

    #[test]
    fn fully_different_images_have_ratio_near_one() {
        let a = solid_png(10, 10, [0, 0, 0, 255]);
        let b = solid_png(10, 10, [255, 255, 255, 255]);

        let outcome = diff_images(&a, &b, &[]).unwrap();

        assert_eq!(outcome.differing_pixels, 100);
        assert_eq!(outcome.ratio, 1.0);
    }

    #[test]
    fn a_mask_over_the_only_difference_absorbs_it_to_zero_ratio() {
        let base = solid_png(10, 10, [0, 0, 0, 255]);
        let changed = png_with_one_pixel(10, 10, [0, 0, 0, 255], (3, 4), [255, 255, 255, 255]);

        let outcome = diff_images(
            &base,
            &changed,
            &[Rect {
                x: 0,
                y: 0,
                w: 5,
                h: 5,
            }],
        )
        .unwrap();

        assert_eq!(outcome.ratio, 0.0);
        assert_eq!(outcome.differing_pixels, 0);
        assert_eq!(outcome.compared_pixels, 100 - 25);
    }

    #[test]
    fn dimension_mismatch_is_a_distinct_error_not_a_diff_ratio() {
        let a = solid_png(10, 10, [0, 0, 0, 255]);
        let b = solid_png(20, 10, [0, 0, 0, 255]);

        let result = diff_images(&a, &b, &[]);

        assert!(matches!(
            result,
            Err(DiffError::DimensionMismatch {
                baseline: (10, 10),
                current: (20, 10)
            })
        ));
    }
}
