use image::{Rgba, imageops};

use crate::imports;
use imports::state2d::{State2D, xy_from_index_u32};
use imports::types::{Colormap, RGBAImageU8};

// --------------------------------------------------------------------------------------------------------------------
// %% Structs

pub struct ThiefData {
    /*
    This struct holds both the source-to-output index mapping as well as the
    order in which source samples were taken.

    These are both used to construct the final displayed image, but neither
    mapping is display/pixel data itself.
    */
    pub thief_map: State2D<Option<usize>>,
    pub order_map: State2D<Option<usize>>,
    iter_count: usize,
}

impl ThiefData {
    pub fn new(out_wh: (usize, usize), src_wh: (usize, usize)) -> Self {
        Self {
            thief_map: State2D::new(None, out_wh.0, out_wh.1),
            order_map: State2D::new(None, src_wh.0, src_wh.1),
            iter_count: 0,
        }
    }

    pub fn clear(&mut self) -> &mut Self {
        self.thief_map.fill(None);
        self.order_map.fill(None);
        self.iter_count = 0;
        return self;
    }

    pub fn resize(&mut self, new_source_wh: (usize, usize), new_output_wh: (usize, usize)) -> &mut Self {
        self.thief_map.resize(new_output_wh.0, new_output_wh.1);
        self.order_map.resize(new_source_wh.0, new_source_wh.1);
        return self;
    }

    pub fn read(&self, output_point: usize) -> Option<usize> {
        /* Reads a point on the output map. Returns the corresponding source sample point if it's been set else None */
        self.thief_map.read(output_point)
    }

    pub fn get_iter_count(&self) -> usize {
        return self.iter_count;
    }

    pub fn record_mapping(&mut self, output_point: usize, source_point: usize) {
        /*
        Records output-to-source pixel index mappings.
        The idea is that every 'pixel' in the output is rendered by 'stealing'
        a pixel from the source image. The mapping is therefore an image-like
        data structure, but instead of holding RGB pixels, it holds the
        associated coordinate from the source image (in the form of a 1d array index).
        */
        debug_assert!(
            self.thief_map.read(output_point).is_none(),
            "Error: Attempting to set an already set output point!"
        );
        self.thief_map.set_state(Some(source_point), output_point);

        // Record ordering
        self.order_map.set_state(Some(self.iter_count), source_point);
        self.iter_count += 1;
    }

    pub fn render_result(
        &self,
        output_image: &mut RGBAImageU8,
        source_image: &RGBAImageU8,
        roll_offset_xy: (f32, f32),
        num_threads: Option<usize>,
    ) {
        /*
        Function used to draw the final output image by sampling from the source
        image according to 'thief_map' that stores a source pixel position for every output pixel position.

        This function is threaded with the thread count being adjustable if needed. It scales somewhat
        poorly (e.g. sub-linear) with more threads, likely due heavily randomized sampling of the source image.

        Note, the output pixels are modified in-place!
        */

        // For clarity
        let roll_x = roll_offset_xy.0.round() as i32;
        let roll_y = roll_offset_xy.1.round() as i32;

        // Figure out roll offsets with wrap-around (this lets us animate in a nice/looping way)
        let ((out_w, out_h), (src_w, src_h)) = (output_image.dimensions(), source_image.dimensions());
        let offset_x = if roll_x >= 0 {
            roll_offset_xy.0 as u32
        } else {
            src_w.saturating_sub(roll_offset_xy.0.abs() as u32)
        };
        let offset_y = if roll_y >= 0 {
            roll_offset_xy.1 as u32
        } else {
            src_h.saturating_sub(roll_offset_xy.1.abs() as u32)
        };

        // Figure out how many threads to use & how many pixels to process per thread
        let num_out_pixels = (out_w * out_h) as usize;
        let n_threads = num_threads.unwrap_or({
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .clamp(1, num_out_pixels)
        });
        let px_per_thread = ((num_out_pixels as f32) / (n_threads as f32)).ceil().max(1.0) as usize;
        let bytes_per_thread = px_per_thread * 4;

        // Split image into separate blocks of pixels, handled by separate threads
        std::thread::scope(|s| {
            for (thread_idx, thread_img_bytes) in output_image.chunks_mut(bytes_per_thread).enumerate() {
                s.spawn(move || {
                    let thread_start_px_idx = thread_idx * px_per_thread;
                    for (px_offset, out_px_bytes) in thread_img_bytes.chunks_exact_mut(4).enumerate() {
                        let out_px_idx = thread_start_px_idx + px_offset;
                        if let Some(src_px_idx) = self.read(out_px_idx) {
                            // Copy source pixel into output
                            let (src_x, src_y) = xy_from_index_u32(src_px_idx as u32, src_w);
                            let x_idx = (src_x + offset_x) % src_w;
                            let y_idx = (src_y + offset_y) % src_h;
                            let src_rgba = source_image.get_pixel(x_idx, y_idx);
                            out_px_bytes[0] = src_rgba[0];
                            out_px_bytes[1] = src_rgba[1];
                            out_px_bytes[2] = src_rgba[2];
                            out_px_bytes[3] = src_rgba[3];
                        }
                    }
                }); // End of spawn block
            }
        }); // End of thread-scope block
    }
}

pub struct ThiefOverlay {
    /* This struct holds data used to visualize the sampling order used to steal pixels from the source image */
    cmap: Colormap,
    overlay: RGBAImageU8,
    has_semitransparency: bool,
}

impl ThiefOverlay {
    pub fn new(colormap: Colormap) -> Self {
        Self {
            cmap: colormap,
            overlay: RGBAImageU8::new(0, 0),
            has_semitransparency: colormap.iter().any(|p| p[3] < 255),
        }
    }

    pub fn clear(&mut self) -> &mut Self {
        self.overlay.fill(0);
        return self;
    }

    pub fn resize(&mut self, new_source_wh: (usize, usize)) {
        self.overlay = RGBAImageU8::from_pixel(new_source_wh.0 as u32, new_source_wh.1 as u32, Rgba([0, 0, 0, 0]));
    }

    pub fn draw_overlay(&mut self, display_image: &mut RGBAImageU8, order_map: &State2D<Option<usize>>) {
        let olay_wh = self.overlay.dimensions();
        let ord_wh = (order_map.width, order_map.height);
        assert!(
            (olay_wh.0 == ord_wh.0 as u32) && (olay_wh.1 == ord_wh.1 as u32),
            "Overlay error! Mismatched sizes ({:?} vs {:?})",
            olay_wh,
            ord_wh
        );

        // Clear existing overlay if using transparent colormap (otherwise we'll 'build up' on re-draws)
        if self.has_semitransparency {
            self.overlay.fill(0);
        }

        // Convert each order index value into a color from the colormap
        let max_idx = (order_map.numel() - 1) as f32;
        for (x, y, idx) in order_map.iter_xy() {
            if let Some(idx) = *idx {
                let idx_1024 = ((idx as f32 / max_idx) * 1023.0).round() as usize;
                let thief_color = self.cmap[idx_1024];
                self.overlay.put_pixel(x as u32, y as u32, thief_color);
            }
        }
        imageops::overlay(display_image, &self.overlay, 0, 0);
    }
}
