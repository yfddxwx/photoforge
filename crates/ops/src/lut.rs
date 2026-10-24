//! `.cube` 3D LUT parsing and application (display-domain, ADR-0002).
//!
//! Format: Adobe Cube LUT spec — `TITLE`, `LUT_3D_SIZE N`, optional
//! `DOMAIN_MIN`/`DOMAIN_MAX`, then N³ rows of `r g b` with **red varying
//! fastest**. Application trilinearly interpolates the encoded sRGB values
//! and maps back to linear.

use photoforge_core::color::srgb;
use photoforge_core::math::{clamp01, lerp};

/// A parsed 3D LUT (table size N, N³ entries, RGB domain [0,1]).
#[derive(Clone, Debug, PartialEq)]
pub struct CubeLut {
    pub name: String,
    pub size: u32,
    /// N³ entries, red fastest varying: idx = (b·N² + g·N)·N + r.
    pub table: Vec<[f32; 3]>,
}

#[derive(Debug)]
pub struct CubeParseError(pub String);

impl std::fmt::Display for CubeParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cube LUT parse error: {}", self.0)
    }
}

impl std::error::Error for CubeParseError {}

impl CubeLut {
    /// Parses the text of a `.cube` file.
    pub fn parse(text: &str) -> Result<Self, CubeParseError> {
        let mut name = String::new();
        let mut size: Option<u32> = None;
        let mut table: Vec<[f32; 3]> = Vec::new();
        for (lineno, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("TITLE") {
                name = rest.trim().trim_matches('"').to_string();
                continue;
            }
            if let Some(rest) = line.strip_prefix("LUT_3D_SIZE") {
                size = Some(rest.trim().parse().map_err(|_| {
                    CubeParseError(format!("line {}: bad LUT_3D_SIZE", lineno + 1))
                })?);
                continue;
            }
            if line.starts_with("DOMAIN_") || line.starts_with("LUT_1D_SIZE") {
                continue; // accepted and ignored (unit domain assumed)
            }
            // Data row.
            let mut it = line.split_whitespace();
            let mut vals = [0.0f32; 3];
            for v in &mut vals {
                *v = it
                    .next()
                    .and_then(|t| t.parse::<f32>().ok())
                    .ok_or_else(|| CubeParseError(format!("line {}: bad float", lineno + 1)))?;
            }
            if it.next().is_some() {
                return Err(CubeParseError(format!(
                    "line {}: extra columns",
                    lineno + 1
                )));
            }
            table.push(vals);
        }
        let size = size.ok_or_else(|| CubeParseError("missing LUT_3D_SIZE".into()))?;
        let expected = (size as usize).pow(3);
        if !(2..=256).contains(&size) {
            return Err(CubeParseError(format!("unreasonable LUT_3D_SIZE {size}")));
        }
        if table.len() != expected {
            return Err(CubeParseError(format!(
                "expected {expected} entries, got {}",
                table.len()
            )));
        }
        Ok(Self { name, size, table })
    }

    /// Reads and parses a `.cube` file.
    pub fn from_file(path: &std::path::Path) -> Result<Self, CubeParseError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| CubeParseError(format!("read failed: {e}")))?;
        Self::parse(&text)
    }

    /// Builds the identity LUT of a given size (for tests and fallbacks).
    pub fn identity(size: u32) -> Self {
        let n3 = (size as usize).pow(3);
        let mut table = Vec::with_capacity(n3);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    table.push([
                        r as f32 / (size - 1) as f32,
                        g as f32 / (size - 1) as f32,
                        b as f32 / (size - 1) as f32,
                    ]);
                }
            }
        }
        Self {
            name: "identity".into(),
            size,
            table,
        }
    }

    /// Samples with trilinear interpolation on encoded values in [0, 1].
    pub fn sample(&self, rgb_encoded: [f32; 3]) -> [f32; 3] {
        sample_table(&self.table, self.size, rgb_encoded)
    }
}

/// Trilinear sampling of an N³ table (red fastest varying).
pub fn sample_table(table: &[[f32; 3]], size: u32, rgb_encoded: [f32; 3]) -> [f32; 3] {
    let n_us = size as usize;
    let last = n_us - 1;
    let x = clamp01(rgb_encoded[0]) * (size as f32 - 1.0);
    let y = clamp01(rgb_encoded[1]) * (size as f32 - 1.0);
    let z = clamp01(rgb_encoded[2]) * (size as f32 - 1.0);
    let x0 = (x.floor() as usize).min(last);
    let y0 = (y.floor() as usize).min(last);
    let z0 = (z.floor() as usize).min(last);
    let x1 = (x0 + 1).min(last);
    let y1 = (y0 + 1).min(last);
    let z1 = (z0 + 1).min(last);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let fz = z - z0 as f32;

    let idx = |r: usize, g: usize, b: usize| (b * n_us + g) * n_us + r;
    let mut out = [0.0f32; 3];
    for c in 0..3 {
        let c00 = lerp(table[idx(x0, y0, z0)][c], table[idx(x1, y0, z0)][c], fx);
        let c10 = lerp(table[idx(x0, y1, z0)][c], table[idx(x1, y1, z0)][c], fx);
        let c01 = lerp(table[idx(x0, y0, z1)][c], table[idx(x1, y0, z1)][c], fx);
        let c11 = lerp(table[idx(x0, y1, z1)][c], table[idx(x1, y1, z1)][c], fx);
        let c0 = lerp(c00, c10, fy);
        let c1 = lerp(c01, c11, fy);
        out[c] = lerp(c0, c1, fz);
    }
    out
}

/// Applies a LUT to a linear-light pixel (encode → sample → decode).
#[inline]
pub fn lut_pixel(px: photoforge_core::Rgba32F, lut: &CubeLut) -> photoforge_core::Rgba32F {
    lut_pixel_table(px, &lut.table, lut.size)
}

/// Table-based variant avoiding a `CubeLut` rebuild per pixel.
#[inline]
pub fn lut_pixel_table(
    px: photoforge_core::Rgba32F,
    table: &[[f32; 3]],
    size: u32,
) -> photoforge_core::Rgba32F {
    let e = [
        srgb::encode_f32(px[0]),
        srgb::encode_f32(px[1]),
        srgb::encode_f32(px[2]),
    ];
    let out = sample_table(table, size, e);
    [
        srgb::decode(out[0]),
        srgb::decode(out[1]),
        srgb::decode(out[2]),
        px[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_lut_is_exact_on_grid() {
        let lut = CubeLut::identity(17);
        for i in 0..=255u8 {
            let e = i as f32 / 255.0;
            let out = lut.sample([e, e, e]);
            for c in out {
                assert!((c - e).abs() < 2e-3, "{e} -> {out:?}");
            }
        }
    }

    #[test]
    fn tiny_lut_known_values() {
        // 2×2×2: corners are the primaries; everything else interpolates.
        let lut = CubeLut {
            name: "test".into(),
            size: 2,
            table: vec![
                [0.0, 0.0, 0.0], // (0,0,0)
                [1.0, 0.0, 0.0], // (1,0,0) red
                [0.0, 1.0, 0.0], // (0,1,0) green
                [1.0, 1.0, 0.0],
                [0.0, 0.0, 1.0], // (0,0,1) blue
                [1.0, 0.0, 1.0],
                [0.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
            ],
        };
        assert_eq!(lut.sample([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
        assert_eq!(lut.sample([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
        // Center = average of all corners.
        let mid = lut.sample([0.5, 0.5, 0.5]);
        for c in mid {
            assert!((c - 0.5).abs() < 1e-6, "mid {mid:?}");
        }
        // Midpoint of the r axis between black and red.
        let m = lut.sample([0.5, 0.0, 0.0]);
        assert!((m[0] - 0.5).abs() < 1e-6 && m[1] == 0.0 && m[2] == 0.0);
    }

    #[test]
    fn parse_standard_file() {
        let text = "\
# a comment
TITLE \"Film look\"
LUT_3D_SIZE 2
DOMAIN_MIN 0.0 0.0 0.0
DOMAIN_MAX 1.0 1.0 1.0
0 0 0
1 0 0
0 1 0
1 1 0
0 0 1
1 0 1
0 1 1
1 1 1
";
        let lut = CubeLut::parse(text).expect("parse");
        assert_eq!(lut.name, "Film look");
        assert_eq!(lut.size, 2);
        assert_eq!(lut.table.len(), 8);
        assert_eq!(lut.table[1], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn parse_errors() {
        assert!(CubeLut::parse("TITLE x\n1 2 3\n").is_err()); // no size
        assert!(CubeLut::parse("LUT_3D_SIZE 2\n0 0 0\n").is_err()); // too few rows
        assert!(CubeLut::parse(
            "LUT_3D_SIZE 2\n0 0 0 9\n0 0 0\n0 0 0\n0 0 0\n0 0 0\n0 0 0\n0 0 0\n0 0 0\n"
        )
        .is_err());
        // Empty lines and comments tolerated.
        let text = format!("# hi\n\nLUT_3D_SIZE 2\n{}", "0 0 0\n".repeat(8));
        assert!(CubeLut::parse(&text).is_ok());
    }

    #[test]
    fn lut_pixel_roundtrips_neutral() {
        let lut = CubeLut::identity(33);
        let px = [0.5f32, 0.25, 0.9, 1.0];
        let out = lut_pixel(px, &lut);
        for c in 0..3 {
            assert!((out[c] - px[c]).abs() < 0.02, "{px:?} -> {out:?}");
        }
    }
}
