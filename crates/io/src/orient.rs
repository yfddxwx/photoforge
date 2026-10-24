//! EXIF Orientation handling.
//!
//! The EXIF `Orientation` tag describes how the *stored* image must be
//! transformed for correct display. photoforge applies it immediately at
//! decode time, so everything downstream works in display orientation and
//! the tag never has to be consulted again.

use photoforge_core::PixelBuffer;
use photoforge_ops::geometry::{flip_h, flip_v, rotate90};

/// The 8 EXIF orientations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Orientation {
    Normal,
    FlipHorizontal,
    Rotate180,
    FlipVertical,
    Transpose,   // flip about the TL→BR diagonal
    Rotate90Cw,  // rotate 90° clockwise to display
    Transverse,  // flip about the TR→BL diagonal
    Rotate90Ccw, // rotate 90° counter-clockwise to display
}

impl Orientation {
    /// Maps the raw tag value (1..=8); unknown values fall back to `Normal`.
    pub fn from_tag(value: u16) -> Self {
        match value {
            2 => Self::FlipHorizontal,
            3 => Self::Rotate180,
            4 => Self::FlipVertical,
            5 => Self::Transpose,
            6 => Self::Rotate90Cw,
            7 => Self::Transverse,
            8 => Self::Rotate90Ccw,
            _ => Self::Normal,
        }
    }

    /// The raw EXIF tag value.
    pub fn tag_value(self) -> u16 {
        match self {
            Self::Normal => 1,
            Self::FlipHorizontal => 2,
            Self::Rotate180 => 3,
            Self::FlipVertical => 4,
            Self::Transpose => 5,
            Self::Rotate90Cw => 6,
            Self::Transverse => 7,
            Self::Rotate90Ccw => 8,
        }
    }
}

/// Applies the orientation transform, returning the display-oriented image.
/// Output dimensions are swapped for the 90° rotations.
pub fn apply_orientation(img: &PixelBuffer, orientation: Orientation) -> PixelBuffer {
    match orientation {
        Orientation::Normal => img.clone(),
        Orientation::FlipHorizontal => flip_h(img),
        Orientation::Rotate180 => rotate90(img, 2),
        Orientation::FlipVertical => flip_v(img),
        // Transpose = rotate 90° CW, then mirror horizontally.
        Orientation::Transpose => flip_h(&rotate90(img, 1)),
        Orientation::Rotate90Cw => rotate90(img, 1),
        // Transverse = rotate 90° CW, then mirror vertically.
        Orientation::Transverse => flip_v(&rotate90(img, 1)),
        Orientation::Rotate90Ccw => rotate90(img, 3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stored 3×2 image: TL red, TR green, BL blue, BR yellow.
    fn marker() -> PixelBuffer {
        let mut img = PixelBuffer::new(3, 2);
        img.set_pixel(0, 0, [255, 0, 0, 255]);
        img.set_pixel(2, 0, [0, 255, 0, 255]);
        img.set_pixel(0, 1, [0, 0, 255, 255]);
        img.set_pixel(2, 1, [255, 255, 0, 255]);
        img
    }

    fn expect_corners(o: Orientation, tl: [u8; 3], tr: [u8; 3], bl: [u8; 3], br: [u8; 3]) {
        let out = apply_orientation(&marker(), o);
        let rgb = |p: [u8; 4]| [p[0], p[1], p[2]];
        let got = [
            rgb(out.pixel(0, 0)),
            rgb(out.pixel(out.width() - 1, 0)),
            rgb(out.pixel(0, out.height() - 1)),
            rgb(out.pixel(out.width() - 1, out.height() - 1)),
        ];
        assert_eq!(got, [tl, tr, bl, br], "{o:?}");
    }

    #[test]
    fn all_eight_orientations_place_corners_correctly() {
        use Orientation::*;
        expect_corners(Normal, [255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]);
        expect_corners(
            FlipHorizontal,
            [0, 255, 0],
            [255, 0, 0],
            [255, 255, 0],
            [0, 0, 255],
        );
        expect_corners(
            Rotate180,
            [255, 255, 0],
            [0, 0, 255],
            [0, 255, 0],
            [255, 0, 0],
        );
        expect_corners(
            FlipVertical,
            [0, 0, 255],
            [255, 255, 0],
            [255, 0, 0],
            [0, 255, 0],
        );
        expect_corners(
            Transpose,
            [255, 0, 0],
            [0, 0, 255],
            [0, 255, 0],
            [255, 255, 0],
        );
        expect_corners(
            Rotate90Cw,
            [0, 0, 255],
            [255, 0, 0],
            [255, 255, 0],
            [0, 255, 0],
        );
        expect_corners(
            Transverse,
            [255, 255, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 0, 0],
        );
        expect_corners(
            Rotate90Ccw,
            [0, 255, 0],
            [255, 255, 0],
            [255, 0, 0],
            [0, 0, 255],
        );
    }

    #[test]
    fn dimensions_swap_for_rotations() {
        let m = marker();
        assert_eq!(apply_orientation(&m, Orientation::Normal).width(), 3);
        assert_eq!(apply_orientation(&m, Orientation::Rotate90Cw).width(), 2);
        assert_eq!(apply_orientation(&m, Orientation::Rotate90Cw).height(), 3);
        assert_eq!(apply_orientation(&m, Orientation::Rotate180).width(), 3);
    }

    #[test]
    fn tag_values_roundtrip_and_sanitize() {
        for v in 1..=8u16 {
            assert_eq!(Orientation::from_tag(v).tag_value(), v);
        }
        assert_eq!(Orientation::from_tag(0), Orientation::Normal);
        assert_eq!(Orientation::from_tag(9), Orientation::Normal);
        assert_eq!(Orientation::from_tag(12345), Orientation::Normal);
    }

    #[test]
    fn double_application_is_not_identity_but_inverse_exists() {
        // Sanity: applying orientation 6 to an already-oriented image once
        // more rotates again (composability), and applying 6 then 8 cancels.
        let m = marker();
        let six = apply_orientation(&m, Orientation::Rotate90Cw);
        let six_eight = apply_orientation(&six, Orientation::Rotate90Ccw);
        assert_eq!(six_eight, m);
    }
}
