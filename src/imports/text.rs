use std::collections::HashMap;

use fontdue::{Font, FontSettings, Metrics};
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
    _raster_cache: HashMap<char, (Metrics, Vec<u8>)>,
    _tallest_char: char,
}

impl FontConfig {
    fn new(size: f32, font_bytes: &[u8]) -> Self {
        let settings = FontSettings {
            collection_index: 0,
            scale: size,
            load_substitutions: false,
        };

        // Initialize with a cache for re-using fontdue render results
        // -> The skips re-rendering characters but also avoids excessive re-allocations
        let raster_cache = HashMap::with_capacity(96);
        let font = Font::from_bytes(font_bytes, settings).unwrap();
        let mut new_self = Self {
            enable_antialias: true,
            y_offset: 0.0,
            font: font,
            size: size,
            _raster_cache: raster_cache,
            _tallest_char: 'Q',
        };

        // Figure out tallest character for setting up the proper baseline offset
        let ascii_iter = (32u8 as char)..127u8 as char;
        let tall_metrics = ascii_iter.map(|c| (c, new_self.rasterize(c).0));
        let (mut tallest_char, mut tallest_baseline) = ('A', 0.0);
        for (c, m) in tall_metrics {
            let char_baseline = m.bounds.height + m.bounds.ymin;
            if char_baseline > tallest_baseline {
                tallest_char = c;
                tallest_baseline = char_baseline;
            }
        }
        new_self.y_offset = tallest_baseline;
        new_self._tallest_char = tallest_char;

        return new_self;
    }

    pub fn rasterize(&mut self, character: char) -> &(Metrics, Vec<u8>) {
        /* Draw character into buffer. Returns: (render_metrics, alpha_map_u8) */
        return self
            ._raster_cache
            .entry(character)
            .or_insert_with(|| self.font.rasterize(character, self.size));
    }
}

// . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .

pub struct TextDrawer {
    pub font_cfg: FontConfig,
    pub text: String,
    color: Rgba<u8>,
}

impl TextDrawer {
    pub fn new(size: f32, font_bytes: &[u8]) -> Self {
        Self {
            font_cfg: FontConfig::new(size, font_bytes),
            color: Rgba([255, 255, 255, 255]),
            text: String::with_capacity(128),
        }
    }

    pub fn new_regular(size: f32) -> Self {
        return Self::new(size, DEFAULT_REGULAR_FONT_BYTES);
    }

    // . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .

    pub fn xy_px(&mut self, image: &mut RGBAImageU8, xy_px: (u32, u32)) {
        /*
        Function used to draw text onto the provided image. Text is drawn left-to-right/top-to-bottom.
        For example, if xy_px = (0, 0), the text will be visible in the top left corner of the image.
        Note, the text is drawn from the current state of the .text buffer! Use: wtxtdraw! to update this.
        */

        let is_antialiased = self.font_cfg.enable_antialias;
        let (img_w, img_h) = image.dimensions();
        let mut next_x_pos = xy_px.0 as f32;
        let y_offset = xy_px.1 as f32 + self.font_cfg.y_offset;
        for character in self.text.chars() {
            let (char_metrics, char_alpha_2d) = self.font_cfg.rasterize(character);
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
                    if is_antialiased && alpha_u8 < 255 {
                        let alpha_norm = alpha_u8 as f32 / 255.0;
                        lerp_colors_mut(pixel_color, self.color, alpha_norm);
                    } else if alpha_u8 > 127 {
                        *pixel_color = self.color;
                    }
                }
            }
        }
    }

    pub fn get_text_size(&mut self) -> (f32, f32, f32) {
        let mut txt_w: f32 = 0.0;
        let mut txt_h: f32 = 0.0;
        let mut txt_baseline: f32 = 0.0;
        for character in self.text.chars() {
            let (char_metrics, _) = self.font_cfg.rasterize(character);
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

macro_rules! wtxtdraw {
	/*
	This is meant to act like the 'write!' macro, but edits the internal string buffer of the text drawer.
	From the user/caller perspective, this macro is like doing:
		txtdraw.text.clear()
		write!(&mut txtdraw.text, "Some data: {} and {}", value_1, value_2);

	This macro just helps to hide the buffer access details, usage is like:
		wtxtdraw!(txtdraw, "Some data: {} and {}", value_1, value_2);
 */
    ($txtdrawer:expr, $($arg:tt)*) => {
        {
            $txtdrawer.text.clear();
            let _ = write!(&mut $txtdrawer.text, $($arg)*);
        }
    };
}
pub(crate) use wtxtdraw;
