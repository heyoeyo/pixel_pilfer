use image::imageops;
use rand::random_range;

// Custom imports
use crate::imports;
use imports::buffer_helpers::{copy_pixels, realloc_image_buffer, resize_and_overlay};
use imports::cli::CliArgs;
use imports::thread_utils::{get_elements_per_thread, get_num_threads};
use imports::types::{BYTES_PER_PIXEL, RGBAImageU8, TWO_PI};

// --------------------------------------------------------------------------------------------------------------------

pub struct PostProcessConfig {
    pub relative_src_size: f32,
    pub hue_rotate: i32,
    pub contrast: f32,
    pub blur: u8,
    pub dirt: u8,
    pub pixelate: u8,
    pub is_grayscale: bool,
}

impl PostProcessConfig {
    pub fn from_cli(args: &CliArgs) -> Self {
        Self {
            relative_src_size: args.source_rel_scale,
            hue_rotate: args.hue,
            contrast: args.contrast.clamp(-100.0, 100.0),
            blur: args.blur,
            dirt: args.dirt,
            pixelate: args.pixelate,
            is_grayscale: false,
        }
    }
}

#[derive(Copy, Clone)]
struct ThreadSharablePointer {
    /*
    This is a special wrapper used for writing image pixel data (e.g. array of u8).
    It's needed purely to allow for writing to pixel data across threads in a way which is 'unsafe'.
    For example, the built-in image type does not allow for 'chunking columns' the way it does for rows,
    so there's no 'safe' way to write to columns from different threads, even though this can be done safely.
    */
    ptr: *mut u8,
}

unsafe impl Send for ThreadSharablePointer {}
impl ThreadSharablePointer {
    pub fn new(pointer: *mut u8) -> Self {
        return Self { ptr: pointer };
    }
    pub unsafe fn write_byte(self, array_index: usize, new_value: u8) {
        /* Overwrites a single byte (e.g. color channel for image data) at the given array index */
        unsafe {
            *self.ptr.add(array_index) = new_value;
        }
    }
}

// --------------------------------------------------------------------------------------------------------------------
// %% Functions

pub fn fast_box_blur(
    inout_image: &mut RGBAImageU8,
    scratch_image: &mut RGBAImageU8,
    intensity: u8,
    num_threads: Option<usize>,
) {
    /*
       Faster (compared to the image-crate) implementation of a box blur.
       This implementation also writes to image data 'in-place'!

       Note that (somewhat strangely) the (blurred) result is stored in the
       'inout' argument, but the function also mutates the 'scratch' input.
       It might be nice to improve the function to not modify it's input in the future...

       This function does a box blur but without a typical 'convolution' approach.
       It still has an implicit sliding window, but it's represented as a running sum.
       Each time the window 'shifts' the trailing pixel value is subtracted from the
       sum, while the incoming pixel value is added. This way we can avoid storing
       the window data explicitly. This also means the execution time is independent
       of the blur size/intensity, only depending on the image size.
    */

    // Sanity check
    debug_assert!(
        scratch_image.dimensions() == inout_image.dimensions(),
        "Box blur error: Mismatched input/scratch image dimensions!"
    );

    // Figure out blur intensity (normalized to image size)
    let (img_w, img_h) = (inout_image.width() as usize, inout_image.height() as usize);
    let min_side = img_w.min(img_h) as f32;
    let intensity_norm = intensity as f32 / 255.0;
    let scaled_side_len = (0.5 * (min_side - 1.0) * intensity_norm.powi(2)).round() as usize;
    let q_size = (1 + 2 * scaled_side_len).max(3);
    let q_halfsize = (q_size / 2) as usize;
    debug_assert!(q_size % 2 == 1, "Box blur error, queue size should be odd!");

    // Pre-compute threading setups
    let n_threads = get_num_threads(num_threads, min_side as u32);
    let rows_per_thread = get_elements_per_thread(img_h as u32, Some(n_threads));
    let cols_per_thread = get_elements_per_thread(img_w as u32, Some(n_threads));

    // Perform blur in two passes (one horizontal, one vertical)
    // -> Each pass uses the same logic, but adjusts 'strides' to get correct sampling
    // -> H-pass works on groups of rows (per thread), v-pass works on groups of columns
    // -> Requires unsafe write, only for column case, but shared for rows due to re-using the logic
    for blur_pass in 0..2 {
        // Figure out sampling pattern for horizontal vs. vertical sampling
        let is_hpass = blur_pass == 0;
        let (num_thread_iters, num_pixel_iters, max_thread_iters): (usize, usize, usize);
        let (stride_per_rowcol, stride_per_pixel): (usize, usize);
        if is_hpass {
            (num_thread_iters, max_thread_iters) = (rows_per_thread, img_h);
            (stride_per_rowcol, stride_per_pixel) = (img_w, 1);
            num_pixel_iters = img_w;
        } else {
            (num_thread_iters, max_thread_iters) = (cols_per_thread, img_w);
            (stride_per_rowcol, stride_per_pixel) = (1, img_w);
            num_pixel_iters = img_h;
        }

        // Do blur pass. Basically: for 'rows or columns' { for pixels in row/col { compute averaged pixel value }}
        let inp_pixels = inout_image.as_raw();
        let out_shared_ptr = ThreadSharablePointer::new(scratch_image.as_mut_ptr());
        std::thread::scope(|s| {
            for thread_idx in 0..n_threads {
                s.spawn(move || {
                    // Set up data and read boundaries for the thread
                    let thread_pxidx_1 = thread_idx * num_thread_iters;
                    let thread_pxidx_2 = (thread_pxidx_1 + num_thread_iters).min(max_thread_iters);
                    let out_thread_ptr = out_shared_ptr;
                    let mut rgba_sums = [0u32; BYTES_PER_PIXEL];

                    // Step along column/row axis (row indices during h-pass, columns during v-pass)
                    for row_or_col_idx in thread_pxidx_1..thread_pxidx_2 {
                        // Compute the first 1D pixel coord for the current row/column (depends on pass)
                        let start_pxidx_offset = row_or_col_idx * stride_per_rowcol;

                        // Fill in initial queue values for the row/column
                        // -> This is computed as if we 'filled the queue' up to the -1 indexed pixel in the rol/col
                        // -> This way the first 'step' below is starting on pixel index 0
                        rgba_sums.fill(0);
                        for qidx in 0..q_size {
                            let px_idx = qidx.saturating_sub(q_halfsize + 1) * stride_per_pixel;
                            let byte_idx = (start_pxidx_offset + px_idx) * BYTES_PER_PIXEL;
                            for chidx in 0..BYTES_PER_PIXEL {
                                rgba_sums[chidx] += inp_pixels[byte_idx + chidx] as u32;
                            }
                        }

                        // Step along pixel axis (along row during h-pass, along column during v-pass)
                        for step_idx in 0..num_pixel_iters {
                            // Figure out which pixel we're writing to (e.g. middle of 'sliding window)
                            let mid_px_idx = step_idx * stride_per_pixel;
                            let mid_byte_idx = (start_pxidx_offset + mid_px_idx) * BYTES_PER_PIXEL;

                            // Figure out pixels that are entering/leaving our sliding window area & add to running sum
                            let new_px_idx = (step_idx + q_halfsize).min(num_pixel_iters - 1) * stride_per_pixel;
                            let old_px_idx = step_idx.saturating_sub(q_halfsize + 1) * stride_per_pixel;
                            let new_byte_idx = (start_pxidx_offset + new_px_idx) * BYTES_PER_PIXEL;
                            let old_byte_idx = (start_pxidx_offset + old_px_idx) * BYTES_PER_PIXEL;

                            // Update output pixel & sum/average per color channel
                            for chidx in 0..BYTES_PER_PIXEL {
                                // Update running sum by removing 'old' and adding 'new' pixel values
                                // -> For example, during h-pass, 'old' is left-most of queue, 'new' is next right-most
                                let new_ch_val = inp_pixels[new_byte_idx + chidx] as u32;
                                let old_ch_val = inp_pixels[old_byte_idx + chidx] as u32;
                                let new_sum = (rgba_sums[chidx] - old_ch_val) + new_ch_val;
                                rgba_sums[chidx] = new_sum;

                                // Write averaged queue color to output
                                // -> During vertical pass, threads are not operating on contiguous 'chunks' so unsafe!
                                let avg_val = (new_sum / q_size as u32) as u8;
                                unsafe {
                                    out_thread_ptr.write_byte(mid_byte_idx + chidx, avg_val);
                                }
                            }
                        }
                    }
                });
            }
        });

        // Here we swap where we read/write from
        // 1st pass: Just wrote to scratch so switching let's us re-use result as input for next pass
        // 2nd pass: Switching again puts final result back into 'inout' (e.g. two swaps: inout->scratch->inout)
        std::mem::swap(inout_image, scratch_image);
    }
}

pub fn dirty_blur(
    input_image: &RGBAImageU8,
    output_image: &mut RGBAImageU8,
    intensity: u8,
    num_threads: Option<usize>,
) {
    /*
    Function which 'blurs' an image by re-sampling pixels within a small randomized circular region.
    The time required to blur is roughly independent of the blur intensity
    */

    debug_assert!(
        output_image.dimensions() == input_image.dimensions(),
        "Dirty blur error: Mismatched input/output image dimensions!"
    );

    // For convenience
    let (img_w, img_h) = input_image.dimensions();
    let (max_x, max_y) = (img_w as i32 - 1, img_h as i32 - 1);
    let radius_px = (intensity as f32 / 255.0).powi(2) * 0.5 * img_w.max(img_h) as f32;

    // Split image into separate blocks of pixels, handled by separate threads
    let px_per_thread = get_elements_per_thread(img_w * img_h, num_threads);
    let bytes_per_thread = px_per_thread * BYTES_PER_PIXEL;
    let in_pixels = input_image.as_raw();
    let out_pixels = output_image.as_mut();
    std::thread::scope(|s| {
        for (thread_idx, thread_img_bytes) in out_pixels.chunks_mut(bytes_per_thread).enumerate() {
            s.spawn(move || {
                // Loop over each pixel within block and sample in random surrounding circle
                let thread_start_px_idx = thread_idx * px_per_thread;
                let img_w_usize = img_w as usize;
                for (px_offset, out_px_bytes) in thread_img_bytes.chunks_mut(BYTES_PER_PIXEL).enumerate() {
                    // Convert the flat index back into (x, y) coordinates
                    let curr_px_idx = thread_start_px_idx + px_offset;
                    let x = curr_px_idx % img_w_usize;
                    let y = curr_px_idx / img_w_usize;

                    // Sample in a circle around each point
                    let sample_radius = random_range(0.0..radius_px);
                    let sample_angle = random_range(0.0..TWO_PI);
                    let dx = ((sample_radius * sample_angle.cos()).round()) as i32;
                    let dy = (sample_radius * sample_angle.sin()).round() as i32;
                    let new_x = (x as i32 + dx).clamp(0, max_x) as u32;
                    let new_y = (y as i32 + dy).clamp(0, max_y) as u32;

                    // Copy original pixel into new output
                    let new_px_idx = (new_x + new_y * img_w) as usize * BYTES_PER_PIXEL;
                    out_px_bytes.copy_from_slice(&in_pixels[new_px_idx..new_px_idx + BYTES_PER_PIXEL]);
                }
            });
        }
    });
}

pub fn postprocess_source_image(
    input_image: &RGBAImageU8,
    output_image: &mut RGBAImageU8,
    scratch_image: &mut RGBAImageU8,
    config: &PostProcessConfig,
) {
    /*
    Applies all post-processing steps on a 'clean' source image.
    Results are written into the 'output_image' input.
    The 'scratch_image' input is used for operations that need 2 buffers and is used to avoid new memory allocations.
    */

    // Apply transformations to produce final image
    copy_pixels(input_image, output_image);
    if config.pixelate > 0 {
        let orig_wh = input_image.dimensions();
        let min_side = orig_wh.0.min(orig_wh.1) as f32;
        let downscale_factor = (config.pixelate as f32 / 255.0).powf(0.25);
        let side_scale_factor = (min_side * (1.0 - downscale_factor) + downscale_factor * 3.0) / min_side;
        let new_w = (orig_wh.0 as f32 * side_scale_factor).round() as u32;
        let new_h = (orig_wh.1 as f32 * side_scale_factor).round() as u32;
        let new_wh = (new_w.max(3), new_h.max(3));
        realloc_image_buffer(scratch_image, new_wh);
        resize_and_overlay(scratch_image, output_image, (0, 0), new_wh, None);
        resize_and_overlay(output_image, scratch_image, (0, 0), orig_wh, None);
        realloc_image_buffer(scratch_image, orig_wh);
    }
    if config.hue_rotate != 0 {
        imageops::colorops::huerotate_in_place(output_image, config.hue_rotate);
    }
    if config.contrast != 0.0 {
        imageops::colorops::contrast_in_place(output_image, config.contrast);

        // Negative contrast adjusts alpha! So restore after adjustment
        // -> This may not look correct on display (doesn't handle alpha), but affects saved images!
        if config.contrast < 0.0 {
            for (x, y, pixel) in output_image.enumerate_pixels_mut() {
                pixel[3] = input_image.get_pixel(x, y)[3];
            }
        }
    }
    if config.blur > 0 {
        // This writes result back into output, but needs a scratch/working buffer (which will be modified!)
        fast_box_blur(output_image, scratch_image, config.blur, None);
    }
    if config.dirt > 0 {
        // Write result into scratch and then flip pointers so we're still working with 'output'
        dirty_blur(output_image, scratch_image, config.dirt, None);
        std::mem::swap(output_image, scratch_image);
    }
    if config.is_grayscale {
        for (_, _, pixel) in output_image.enumerate_pixels_mut() {
            let avg_luma = ((pixel[0] as u32) * 21 + (pixel[1] as u32) * 72 + (pixel[2] as u32) * 7) / 100;
            let luma_u8 = avg_luma.min(255) as u8;
            pixel[0] = luma_u8;
            pixel[1] = luma_u8;
            pixel[2] = luma_u8;
        }
    }
}

pub fn get_source_wh(input_wh: (u32, u32), output_wh: (usize, usize), relative_source_size: f32) -> (u32, u32) {
    /*
    Helper used to figure out the sizing of the source image, so that it has the correct
    number of pixels to match the output image (with possible under/oversizing)
    Returns:
        source_wh
    */

    // For convenience
    let (inp_w, inp_h) = input_wh;
    let (num_src_pixels, num_out_pixels) = (inp_w * inp_h, (output_wh.0 * output_wh.1) as u32);

    // Here we compute the theoretical sizing of the source image with oversizing/undersizing
    let resize_ratio = (relative_source_size * (num_out_pixels as f32) / (num_src_pixels as f32)).sqrt();
    let new_w_f32 = inp_w as f32 * resize_ratio;
    let new_h_f32 = inp_h as f32 * resize_ratio;
    let new_wh_rounded = (new_w_f32.round() as u32, new_h_f32.round() as u32);
    let num_resize_pixels = new_wh_rounded.0 * new_wh_rounded.1;

    // Figure out resizing dimensions to use (we want num source pixels >= output pixel, unless undersizing)
    let is_undersizing = relative_source_size < 1.0;
    let has_enough_resize_pixels = (num_resize_pixels >= num_out_pixels) && !is_undersizing;
    let resize_wh: (u32, u32);
    if is_undersizing || has_enough_resize_pixels {
        resize_wh = new_wh_rounded;
    } else {
        // In the no-oversize (or small oversize) case, we need source pixels >= output pixels
        // -> We need pixel sizes, which involves rounding
        // -> Rounding may lead to having fewer source pixels than output pixels,
        //    so here we check different rounding variations to find closest resize
        //    that also has as many or more
        let (ceilw, ceilh) = (new_w_f32.ceil(), new_h_f32.ceil());
        let new_wh_ceilw = (ceilw, (ceilw * (inp_h as f32 / inp_w as f32)).ceil());
        let new_wh_ceilh = ((ceilh * (inp_w as f32 / inp_h as f32)).ceil(), ceilh);
        let new_wh_ceilwh = (ceilw, ceilh);
        let (new_w_fit, new_h_fit) = [new_wh_ceilw, new_wh_ceilh, new_wh_ceilwh]
            .iter()
            .map(|(w, h)| (*w as u32, *h as u32))
            .filter(|(w, h)| (w * h) >= num_out_pixels)
            .min_by_key(|(w, h)| w * h)
            .unwrap();
        resize_wh = (new_w_fit, new_h_fit);
    }

    return resize_wh;
}
