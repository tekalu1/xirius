//! 色の型と、コントラスト比の計算（WCAG 2.x）。

/// sRGB の 8 ビット + アルファ。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// `0xRRGGBB`（不透明）。
    pub const fn hex(rgb: u32) -> Self {
        Self {
            r: (rgb >> 16) as u8,
            g: (rgb >> 8) as u8,
            b: rgb as u8,
            a: 255,
        }
    }

    /// `0xRRGGBB` とアルファ（0〜255）。
    pub const fn hex_alpha(rgb: u32, a: u8) -> Self {
        Self {
            r: (rgb >> 16) as u8,
            g: (rgb >> 8) as u8,
            b: rgb as u8,
            a,
        }
    }

    /// `0xRRGGBB`（アルファは含まない）。
    pub const fn to_rgb_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    /// `0xRRGGBBAA`。GPUI の `rgba()` にそのまま渡せる並び。
    pub const fn to_rgba_u32(self) -> u32 {
        (self.to_rgb_u32() << 8) | self.a as u32
    }

    /// 0.0〜1.0 の `[r, g, b, a]`。
    pub fn to_f32(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a].map(|c| f32::from(c) / 255.0)
    }

    pub fn is_opaque(self) -> bool {
        self.a == 255
    }

    /// `background`（不透明）の上に重ねた結果の色。
    pub fn over(self, background: Color) -> Color {
        let a = f32::from(self.a) / 255.0;
        let mix = |fg: u8, bg: u8| (f32::from(fg) * a + f32::from(bg) * (1.0 - a)).round() as u8;
        Color {
            r: mix(self.r, background.r),
            g: mix(self.g, background.g),
            b: mix(self.b, background.b),
            a: 255,
        }
    }

    /// WCAG の相対輝度（不透明として計算する）。
    pub fn relative_luminance(self) -> f64 {
        fn lin(c: u8) -> f64 {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }
}

/// WCAG のコントラスト比（1.0〜21.0）。
pub fn contrast_ratio(a: Color, b: Color) -> f64 {
    let (la, lb) = (a.relative_luminance(), b.relative_luminance());
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// 文字（アイコンを含む）に要るコントラスト比。
pub const MIN_TEXT_CONTRAST: f64 = 4.5;
/// 文字ではない図形（ノート・波形・メーター・フォーカスの輪）に要るコントラスト比。
pub const MIN_GRAPHIC_CONTRAST: f64 = 3.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_on_white_is_21() {
        let r = contrast_ratio(Color::hex(0x000000), Color::hex(0xffffff));
        assert!((r - 21.0).abs() < 1e-9);
    }

    #[test]
    fn same_color_is_1() {
        let c = Color::hex(0x336699);
        assert!((contrast_ratio(c, c) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn over_blends() {
        let fg = Color::hex_alpha(0xffffff, 128);
        let out = fg.over(Color::hex(0x000000));
        assert_eq!(out.to_rgb_u32(), 0x808080);
    }

    #[test]
    fn rgba_layout() {
        assert_eq!(Color::hex_alpha(0x112233, 0x44).to_rgba_u32(), 0x1122_3344);
    }
}
