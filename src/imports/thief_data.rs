use crate::imports;
use imports::state2d::{State2D, xy_from_index, xy_from_index_u32};
use imports::thread_utils::get_elements_per_thread;
use imports::types::{BYTES_PER_PIXEL, Colormap, RGBAImageU8};

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
    pub src_sample_order: Vec<usize>,
    iter_count: usize,
}

impl ThiefData {
    pub fn new(out_wh: (usize, usize), src_wh: (usize, usize)) -> Self {
        Self {
            thief_map: State2D::new(None, out_wh.0, out_wh.1),
            src_sample_order: Vec::with_capacity(src_wh.0 * src_wh.1),
            iter_count: 0,
        }
    }

    pub fn clear(&mut self) -> &mut Self {
        self.thief_map.fill(None);
        self.src_sample_order.clear();
        self.iter_count = 0;
        return self;
    }

    pub fn resize(&mut self, new_source_wh: (usize, usize), new_output_wh: (usize, usize)) -> &mut Self {
        self.thief_map.resize(new_output_wh.0, new_output_wh.1);
        self.src_sample_order.resize(new_source_wh.0 * new_source_wh.1, 0);
        self.src_sample_order.clear();
        return self;
    }

    pub fn read(&self, output_point: usize) -> Option<usize> {
        /* Reads a point on the output map. Returns the corresponding source sample point if it's been set else None */
        self.thief_map.read(output_point)
    }

    pub fn get_slice(&self, index: usize, length: usize) -> &[Option<usize>] {
        return self.thief_map.get_slice(index, length);
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
            "Error: Attempting to set an already set output point! ({output_point})"
        );
        self.thief_map.set_state(Some(source_point), output_point);

        // Record sample ordering
        self.src_sample_order.push(source_point);
        self.iter_count += 1;
    }

    pub fn render_result(
        &self,
        output_image: &mut RGBAImageU8,
        source_image: &RGBAImageU8,
        roll_offset_xy: (f32, f32),
        use_nearest_sampling: bool,
        num_threads: Option<usize>,
    ) {
        if use_nearest_sampling {
            self._render_nearest(output_image, source_image, roll_offset_xy, num_threads);
        } else {
            self._render_bilinear(output_image, source_image, roll_offset_xy, num_threads);
        }
    }

    fn _render_nearest(
        &self,
        render_buffer: &mut RGBAImageU8,
        source_image: &RGBAImageU8,
        roll_offset_xy: (f32, f32),
        num_threads: Option<usize>,
    ) {
        /*
        Rendering helper for the thief map. This version uses nearest neighbor sampling
        when handling offsets. This runs much faster than the bilinear-sampling counterpart,
        but cannot smoothly animate fractional roll offsets. Meant for quick interactive use.
        */

        // For clarity
        let (out_w, out_h) = render_buffer.dimensions();
        let (src_w, src_h) = source_image.dimensions();

        // Convert offsets to positive values only
        let roll_x = roll_offset_xy.0.round() as i32;
        let roll_y = roll_offset_xy.1.round() as i32;
        let offset_x = if roll_x >= 0 {
            roll_x as u32
        } else {
            src_w.saturating_add_signed(roll_x)
        };
        let offset_y = if roll_y >= 0 {
            roll_y as u32
        } else {
            src_h.saturating_add_signed(roll_y)
        };

        // Break output into equal sized chunks per thread
        let px_per_thread = get_elements_per_thread(out_w * out_h, num_threads);
        let bytes_per_thread = px_per_thread * BYTES_PER_PIXEL;
        let src_pixel_data = source_image.as_raw();
        std::thread::scope(|s| {
            for (thread_idx, thread_img_bytes) in render_buffer.chunks_mut(bytes_per_thread).enumerate() {
                let tdata = self.get_slice(thread_idx * px_per_thread, px_per_thread);
                s.spawn(move || {
                    for (px_offset, out_px_bytes) in thread_img_bytes.chunks_mut(BYTES_PER_PIXEL).enumerate() {
                        if let Some(src_px_idx) = tdata[px_offset] {
                            // Copy source pixel into output
                            let (src_x, src_y) = xy_from_index_u32(src_px_idx as u32, src_w);
                            let x_idx = (src_x + offset_x) % src_w;
                            let y_idx = (src_y + offset_y) % src_h;
                            let new_idx = (x_idx + y_idx * src_w) as usize * BYTES_PER_PIXEL;
                            out_px_bytes.copy_from_slice(&src_pixel_data[new_idx..new_idx + BYTES_PER_PIXEL]);
                        }
                    }
                });
            }
        });
    }

    fn _render_bilinear(
        &self,
        render_buffer: &mut RGBAImageU8,
        source_image: &RGBAImageU8,
        roll_offset_xy: (f32, f32),
        num_threads: Option<usize>,
    ) {
        /*
        Render thief map with bilinear filtering to support fractional roll offsets.
        This is a sister function to the 'nearest neighbor' rendering. This runs ~3-4x
        slower, but leads to smoother animations (meant for video recording).
        */

        // For clarity
        let (out_w, out_h) = render_buffer.dimensions();
        let (src_w, src_h) = source_image.dimensions();

        // Figure out (positive-only) xy offsets
        let (offset_x_f32, offset_y_f32) = roll_offset_xy;
        let (off_x0, off_x1) = if offset_x_f32 >= 0.0 {
            (offset_x_f32.floor() as u32, offset_x_f32.ceil() as u32)
        } else {
            // Wrap offsets around to 0-to-src_w range. Note for negative number ceil rounds away from zero!
            let upper = src_w - offset_x_f32.abs().floor() as u32;
            (upper, upper - 1)
        };
        let (off_y0, off_y1) = if offset_y_f32 >= 0.0 {
            (offset_y_f32.floor() as u32, offset_y_f32.ceil() as u32)
        } else {
            let lower = src_h - offset_y_f32.abs().ceil() as u32;
            (lower, lower + 1)
        };
        let (fract_x, fract_y) = (offset_x_f32.abs().fract(), offset_y_f32.abs().fract());
        let src_stride = src_w as u32;

        // Break output into equal sized chunks per thread
        let px_per_thread = get_elements_per_thread(out_w * out_h, num_threads);
        let bytes_per_thread = px_per_thread * BYTES_PER_PIXEL;
        let src_pixel_data = source_image.as_raw();
        std::thread::scope(|s| {
            for (thread_idx, thread_img_bytes) in render_buffer.chunks_mut(bytes_per_thread).enumerate() {
                let tdata = self.get_slice(thread_idx * px_per_thread, px_per_thread);
                s.spawn(move || {
                    for (px_offset, out_px_bytes) in thread_img_bytes.chunks_mut(BYTES_PER_PIXEL).enumerate() {
                        if let Some(src_px_idx) = tdata[px_offset] {
                            let (src_x, src_y) = xy_from_index_u32(src_px_idx as u32, src_w);

                            // Get 'top-left, bottom-right' xy coordinates for bilinear sampling
                            let x0 = (src_x + off_x0) % src_w;
                            let x1 = (src_x + off_x1) % src_w;
                            let y0 = (src_y + off_y0) % src_h;
                            let y1 = (src_y + off_y1) % src_h;

                            // Compute byte index offsets from xy coords
                            let (x0_pxoff, y0_pxoff) = (x0, y0 * src_stride);
                            let (x1_pxoff, y1_pxoff) = (x1, y1 * src_stride);
                            let byte_00 = (x0_pxoff + y0_pxoff) as usize * BYTES_PER_PIXEL;
                            let byte_10 = (x1_pxoff + y0_pxoff) as usize * BYTES_PER_PIXEL;
                            let byte_01 = (x0_pxoff + y1_pxoff) as usize * BYTES_PER_PIXEL;
                            let byte_11 = (x1_pxoff + y1_pxoff) as usize * BYTES_PER_PIXEL;

                            // Perform bilinear averaging on each channel and store as output
                            for chidx in 0..BYTES_PER_PIXEL {
                                let px_00 = src_pixel_data[byte_00 + chidx] as f32;
                                let px_10 = src_pixel_data[byte_10 + chidx] as f32;
                                let px_01 = src_pixel_data[byte_01 + chidx] as f32;
                                let px_11 = src_pixel_data[byte_11 + chidx] as f32;

                                // Bilinear blend horizontally on top & bottom, then vertically
                                let blend_top = px_00 + fract_x * (px_10 - px_00);
                                let blend_bot = px_01 + fract_x * (px_11 - px_01);
                                let final_val = blend_top + fract_y * (blend_bot - blend_top);
                                out_px_bytes[chidx] = final_val as u8;
                            }
                        }
                    }
                });
            }
        });
    }
}

pub struct ThiefOverlay {
    /* This struct helps visualize the sampling order used to steal pixels from the source image */
    cmap: Colormap,
    last_iter_idx: usize,
}

impl ThiefOverlay {
    pub fn new(colormap: Colormap) -> Self {
        Self {
            cmap: colormap,
            last_iter_idx: 0,
        }
    }

    pub fn clear(&mut self) {
        self.last_iter_idx = 0;
    }

    pub fn draw_overlay(
        &mut self,
        source_display_image: &mut RGBAImageU8,
        source_sample_order: &Vec<usize>,
        iteration_count: usize,
    ) {
        /*
        Function used to draw a 'sampling order' overlay on top of the provided image,
        which helps visualize how pixels are taken from the source image.

        For the sake of efficiency, this function does not re-draw the entire overlay
        on every call, only the newest samples (since the last call).
        Therefore the display image is expected to be re-used between calls
        */

        // Set up scaling factors to map from sampling order to color map entries
        let sample_scale = 1.0 / (source_sample_order.capacity() - 1) as f32;
        let cmap_max_idx = (self.cmap.len() - 1) as f32;

        // Draw new (since last call) samples colormapped by sample order, directly into the image
        let mut sample_idx = self.last_iter_idx as f32;
        let src_w = source_display_image.width() as usize;
        for src_idx in &source_sample_order[self.last_iter_idx..] {
            let cmap_idx = (sample_idx * sample_scale * cmap_max_idx).round() as usize;
            let thief_color = self.cmap[cmap_idx];
            let (x, y) = xy_from_index(*src_idx, src_w);
            source_display_image.put_pixel(x as u32, y as u32, thief_color);
            sample_idx += 1.0;
        }

        // Record last index for next call (we skip previously drawn points)
        self.last_iter_idx = iteration_count;
    }
}
