use image::imageops;
use rand::random_range;

// Custom imports
use crate::imports;
use imports::buffer_helpers::resize_image;
use imports::cli::CliArgs;
use imports::types::RGBAImageU8;

// --------------------------------------------------------------------------------------------------------------------

const TWO_PI: f32 = 2.0 * std::f32::consts::PI;

// --------------------------------------------------------------------------------------------------------------------

pub struct PostProcessConfig {
    pub relative_src_size: f32,
    pub hue_rotate: i32,
    pub contrast: f32,
    pub blur: u8,
    pub dirt: u8,
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
            is_grayscale: false,
        }
    }
}

// --------------------------------------------------------------------------------------------------------------------
// %% Functions

pub fn normalized_blur(image: &RGBAImageU8, intensity: u8) -> RGBAImageU8 {
    /* Helper used to perform fast gaussian blur with simplified 0-to-255 intensity control */
    const MAX_BLUR_SCALE: f32 = 0.1;
    let sigma_norm = (intensity as f32 / 255.0).powi(2);
    let (img_w, img_h) = image.dimensions();
    let max_side_px = img_w.max(img_h) as f32;
    let sigma_px = (sigma_norm * max_side_px) * MAX_BLUR_SCALE;
    return imageops::fast_blur(&image, sigma_px);
}

pub fn dirty_blur(image: &RGBAImageU8, intensity: u8, num_threads: Option<usize>) -> RGBAImageU8 {
    /*
    Function which 'blurs' an image by re-sampling pixels within a small randomized circular region.
    The time required to blur is roughly independent of the blur intensity
    */

    // For convenience
    let (img_w, img_h) = image.dimensions();
    let (max_x, max_y) = (img_w as i32 - 1, img_h as i32 - 1);
    let radius_px = (intensity as f32 / 255.0).powi(2) * 0.5 * img_w.max(img_h) as f32;
    let num_out_pixels = (img_w * img_h) as usize;

    // Figure out how many threads to use
    let n_threads = num_threads.unwrap_or({
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, num_out_pixels)
    });
    let px_per_thread = ((num_out_pixels as f32) / (n_threads as f32)).ceil().max(1.0) as usize;
    let bytes_per_thread = px_per_thread * 4;

    // Split image into separate blocks of pixels, handled by separate threads
    let in_pixel_data = image.as_raw();
    let mut out_pixels = vec![0u8; (num_out_pixels * 4) as usize];
    std::thread::scope(|s| {
        for (thread_idx, thread_img_bytes) in out_pixels.chunks_mut(bytes_per_thread).enumerate() {
            s.spawn(move || {
                // Loop over each pixel within block and sample in random surrounding circle
                let thread_start_px_idx = thread_idx * px_per_thread;
                let img_w_usize = img_w as usize;
                for (px_offset, out_px_bytes) in thread_img_bytes.chunks_exact_mut(4).enumerate() {
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
                    let new_px_idx = ((new_x + new_y * img_w) * 4) as usize;
                    out_px_bytes[0..4].copy_from_slice(&in_pixel_data[new_px_idx..new_px_idx + 4]);
                }
            });
        }
    });

    return RGBAImageU8::from_raw(img_w, img_h, out_pixels).expect("Dirty blur error! Mismatched image sizing");
}

pub fn postprocess_source_image(source_image: &RGBAImageU8, config: &PostProcessConfig) -> RGBAImageU8 {
    /* Applies all post-processing steps on a 'clean' source image */

    // Resize & process source image as needed
    let mut new_src_buffer = source_image.clone();
    if config.hue_rotate != 0 {
        imageops::colorops::huerotate_in_place(&mut new_src_buffer, config.hue_rotate);
    }
    if config.contrast != 0.0 {
        if config.contrast > 0.0 {
            imageops::colorops::contrast_in_place(&mut new_src_buffer, config.contrast);
        } else {
            // Negative contrast adjusts alpha! So restore original value after adjustment
            let orig_alpha: Vec<u8> = new_src_buffer.pixels().map(|p| p[3]).collect();
            imageops::colorops::contrast_in_place(&mut new_src_buffer, config.contrast);
            for (idx, p) in new_src_buffer.pixels_mut().enumerate() {
                p[3] = orig_alpha[idx];
            }
        }
    }
    if config.blur > 0 {
        new_src_buffer = normalized_blur(&new_src_buffer, config.blur);
    }
    if config.dirt > 0 {
        new_src_buffer = dirty_blur(&new_src_buffer, config.dirt, None);
    }
    if config.is_grayscale {
        for (_, _, pixel) in new_src_buffer.enumerate_pixels_mut() {
            let avg_luma = ((pixel[0] as u32) * 21 + (pixel[1] as u32) * 72 + (pixel[2] as u32) * 7) / 100;
            let luma_u8 = avg_luma.min(255) as u8;
            pixel[0] = luma_u8;
            pixel[1] = luma_u8;
            pixel[2] = luma_u8;
        }
    }

    return new_src_buffer;
}

pub fn prepare_sized_image_data(
    output_image: &mut RGBAImageU8,
    loaded_source_image: &RGBAImageU8,
    output_wh: (usize, usize),
    relative_source_size: f32,
) {
    /*
    Helper used to create initial source buffer, sized appropriately for mapping
    source pixels to output pixels (with possible under/oversizing)
    */

    // For convenience
    let src_wh = loaded_source_image.dimensions();
    let (num_src_pixels, num_out_pixels) = (src_wh.0 * src_wh.1, (output_wh.0 * output_wh.1) as u32);

    // Here we compute the theoretical sizing of the source image with oversizing/undersizing
    let resize_ratio = (relative_source_size * (num_out_pixels as f32) / (num_src_pixels as f32)).sqrt();
    let new_w_f32 = src_wh.0 as f32 * resize_ratio;
    let new_h_f32 = src_wh.1 as f32 * resize_ratio;
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
        let new_wh_ceilw = (ceilw, (ceilw * (src_wh.1 as f32 / src_wh.0 as f32)).ceil());
        let new_wh_ceilh = ((ceilh * (src_wh.0 as f32 / src_wh.1 as f32)).ceil(), ceilh);
        let new_wh_ceilwh = (ceilw, ceilh);
        let (new_w_fit, new_h_fit) = [new_wh_ceilw, new_wh_ceilh, new_wh_ceilwh]
            .iter()
            .map(|(w, h)| (*w as u32, *h as u32))
            .filter(|(w, h)| (w * h) >= num_out_pixels)
            .min_by_key(|(w, h)| w * h)
            .unwrap();
        resize_wh = (new_w_fit, new_h_fit);
    }

    // Resize input and store into output
    resize_image(loaded_source_image, output_image, resize_wh, None);
}
