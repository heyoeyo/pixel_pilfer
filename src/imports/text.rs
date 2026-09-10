use fontdue::{Font, FontSettings};
use image::Rgba;

use crate::imports;
use imports::types::RGBAImageU8;

// --------------------------------------------------------------------------------------------------------------------

const DEFAULT_REGULAR_FONT_BYTES: &[u8] = include_bytes!("../resources/SourceCodePro-Regular-shrunk.ttf");

// --------------------------------------------------------------------------------------------------------------------

pub struct FontConfig {
    pub enable_antialias: bool,
    pub y_offset: f32,
    font: Font,
    size: f32,
    _tallest_char: char,
}

impl FontConfig {
    fn new(size: f32, font_bytes: &[u8]) -> Self {
        let settings = FontSettings {
            collection_index: 0,
            scale: size,
            load_substitutions: false,
        };

        // Figure out tallest character for setting up the proper baseline offset
        let font = Font::from_bytes(font_bytes, settings).unwrap();
        let tall_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let tall_metrics = tall_chars.chars().map(|c| (c, font.rasterize(c, size).0));
        let (mut tallest_char, mut tallest_baseline) = ('A', 0.0);
        for (c, m) in tall_metrics {
            let char_baseline = m.bounds.height + m.bounds.ymin;
            if char_baseline > tallest_baseline {
                tallest_char = c;
                tallest_baseline = char_baseline;
            }
        }

        Self {
            enable_antialias: true,
            y_offset: tallest_baseline,
            font: font,
            size: size,
            _tallest_char: tallest_char,
        }
    }
}

// . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .

pub struct TextDrawer {
    pub font_cfg: FontConfig,
    color: Rgba<u8>,
}

impl TextDrawer {
    pub fn new(size: f32, font_bytes: &[u8]) -> Self {
        Self {
            font_cfg: FontConfig::new(size, font_bytes),
            color: Rgba([255, 255, 255, 255]),
        }
    }

    pub fn new_regular(size: f32) -> Self {
        return Self::new(size, DEFAULT_REGULAR_FONT_BYTES);
    }

    // . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .

    pub fn xy_px(&self, image: &mut RGBAImageU8, text: &str, xy_px: (u32, u32)) {
        /*
        Function used to draw text onto the provided image.
        Text is drawn left-to-right/top-to-bottom.
        For example, if xy_px = (0, 0), the text will be visible in the top left corner of the image
        */

        let (img_w, img_h) = image.dimensions();
        let mut next_x_pos = xy_px.0 as f32;
        let y_offset = xy_px.1 as f32 + self.font_cfg.y_offset;
        for character in text.chars() {
            let (char_metrics, char_alpha_2d) = self.font_cfg.font.rasterize(character, self.font_cfg.size);
            let char_x = (next_x_pos + char_metrics.bounds.xmin).round() as u32;
            let char_y = (y_offset - char_metrics.bounds.ymin - char_metrics.bounds.height).round() as u32;
            next_x_pos += char_metrics.advance_width;

            // Some characters (e.g. space) don't have anything to draw!
            if char_metrics.width == 0 {
                continue;
            }

            // Draw text, pixel-by-pixel, into the image
            for (row_idx, row) in char_alpha_2d.chunks_exact(char_metrics.width).enumerate() {
                // Skip drawing if we go off the bottom of the image
                let pixel_y = char_y + row_idx as u32;
                if pixel_y >= img_h {
                    break;
                }

                for (col_idx, &alpha_u8) in row.iter().enumerate() {
                    // Skip drawing for see-through pixels
                    if alpha_u8 == 0 {
                        continue;
                    }

                    // Skip drawing if this pixel is off the right side of the image
                    let pixel_x = char_x + col_idx as u32;
                    if pixel_x >= img_w {
                        continue;
                    }

                    // Overwrite image pixel or blend for anti-aliasing effect
                    let pixel_color = image.get_pixel_mut(pixel_x, pixel_y);
                    if self.font_cfg.enable_antialias && alpha_u8 < 255 {
                        let alpha_norm = alpha_u8 as f32 / 255.0;
                        lerp_colors_mut(pixel_color, self.color, alpha_norm);
                    } else if alpha_u8 > 127 {
                        *pixel_color = self.color;
                    }
                }
            }
        }
    }

    pub fn get_text_size(&self, text: &str) -> (f32, f32, f32) {
        let mut txt_w: f32 = 0.0;
        let mut txt_h: f32 = 0.0;
        let mut txt_baseline: f32 = 0.0;
        for character in text.chars() {
            let (char_metrics, _) = self.font_cfg.font.rasterize(character, self.font_cfg.size);
            txt_w += char_metrics.advance_width;
            txt_h = txt_h.max(char_metrics.bounds.height);
            txt_baseline = txt_baseline.max(char_metrics.bounds.ymin * -1.0);
        }
        return (txt_w, txt_h, txt_baseline);
    }

    pub fn get_font_size(&self) -> f32 {
        return self.font_cfg.size;
    }
}

fn lerp_colors_mut(c_out: &mut Rgba<u8>, c_in: Rgba<u8>, alpha_norm: f32) {
    let inv_alpha = 1.0 - alpha_norm;
    c_out.0[0] = (c_in.0[0] as f32 * alpha_norm + c_out.0[0] as f32 * inv_alpha) as u8;
    c_out.0[1] = (c_in.0[1] as f32 * alpha_norm + c_out.0[1] as f32 * inv_alpha) as u8;
    c_out.0[2] = (c_in.0[2] as f32 * alpha_norm + c_out.0[2] as f32 * inv_alpha) as u8;
}
