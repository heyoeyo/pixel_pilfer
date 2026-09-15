use crate::imports::types;
use types::RGBAImageU8;

// --------------------------------------------------------------------------------------------------------------------

const BYTES_PER_PIXEL: usize = 4;

// --------------------------------------------------------------------------------------------------------------------
// Functions

pub fn realloc_image_buffer(image_buffer: &mut RGBAImageU8, new_wh: (u32, u32)) -> bool {
    /*
    Function used to resize an image buffer without allocating memory, if possible (e.g. when downsizing).
    This is similar to resizing vectors, but works on image buffers.
    Returns:
        needed_memory_allocation
    */

    // Handle re-allocating (if we need a bigger buffer) or re-using memory for smaller buffers
    let current_capacity = image_buffer.as_raw().capacity();
    let required_capacity = (new_wh.0 * new_wh.1 * BYTES_PER_PIXEL as u32) as usize;
    let need_memory_allocation = required_capacity > current_capacity;
    if need_memory_allocation {
        *image_buffer = RGBAImageU8::new(new_wh.0, new_wh.1);
    } else {
        // Re-use the underlying pixel data (vector) to store the new smaller image. Looks a bit odd...
        // -> The underlying vector belongs to an image struct, which belongs to image_buffer
        // -> We want to create a new (smaller) image struct which owns the original vector
        // -> So first need to break image_buffer<->image_struct ownership, then break image_struct<->vector ownership
        let old_buffer_struct = std::mem::take(image_buffer);
        let mut underlying_vector: Vec<u8> = old_buffer_struct.into_raw();
        underlying_vector.resize(required_capacity, 0);
        *image_buffer = RGBAImageU8::from_raw(new_wh.0, new_wh.1, underlying_vector).unwrap();
    }

    return need_memory_allocation;
}

pub fn init_buffer_size(image_buffer: &mut RGBAImageU8, display_wh: (u32, u32), max_wh: (u32, u32)) {
    /*
    Helper used to setup image buffer based on a maximize size and display size.
    The max size is used to allocate a 'max' amount of memory, while the buffer
    is sized to only use the display size amount.
    This is purely to minimize allocations for data that may be resized frequently.
    Note also the 'max' amount is not a hard limit (the buffer can still be made bigger)
    */
    realloc_image_buffer(image_buffer, max_wh);
    realloc_image_buffer(image_buffer, display_wh);
}

pub fn resize_and_overlay(
    output_image: &mut RGBAImageU8,
    input_image: &RGBAImageU8,
    xy_position: (i32, i32),
    new_wh: (u32, u32),
    num_threads: Option<usize>,
) {
    /*
    Helper function used to quickly resize and 'copy' one image onto another.
    This is basically the same as doing:
      let scaled_img = imageops::resize(input_image, width, height, 'nearest')
      imageops::overlay(output_image, scaled_img, x, y);

    This version is *significantly* faster than the image crate implementations.
    However, transparency (e.g. using a see-through input image) is not supported
    */

    // For clarity
    let (inp_w, inp_h) = (input_image.width() as usize, input_image.height() as usize);
    let (out_w, out_h) = (output_image.width() as usize, output_image.height() as usize);
    let (x_pos, y_pos) = xy_position;
    let (resize_w, resize_h) = (new_wh.0 as usize, new_wh.1 as usize);

    // Figure out column indexing for output image
    let out_x1 = x_pos.clamp(0, out_w as i32) as usize;
    let out_x2 = (resize_w as i32 + x_pos).clamp(0, out_w as i32) as usize;
    let num_columns_to_copy = out_x2.saturating_sub(out_x1);
    if num_columns_to_copy == 0 {
        return;
    }

    // Figure out row indexing for output image
    let out_y1 = y_pos.clamp(0, out_h as i32) as usize;
    let out_y2 = (resize_h as i32 + y_pos).clamp(0, out_h as i32) as usize;
    let num_rows_to_copy = out_y2.saturating_sub(out_y1);
    if num_rows_to_copy == 0 {
        return;
    }

    // Pre-compute resized xy-indexing with byte/px & dimension scaling
    // -> This is a significant optimization, but looks a bit mysterious
    // -> We're just pre-computing which input-pixels to sample from, for every row/column of the resized image
    // -> To make things more confusing, we're also working with 1D 'byte' indexing, not 2D pixels!
    let (inp_max_w, inp_max_h) = ((inp_w - 1) as f32, (inp_h - 1) as f32);
    let (rsz_max_w, rsz_max_h) = ((resize_w - 1) as f32, (resize_h - 1) as f32);
    let (rsz_x1, rsz_y1) = (0.max(-x_pos) as usize, 0.max(-y_pos) as usize);
    let full_x_pxidx: Vec<usize> = (rsz_x1..(rsz_x1 + num_columns_to_copy))
        .map(|x| x as f32 / rsz_max_w)
        .map(|xnorm| (xnorm * inp_max_w).round() as usize)
        .map(|x_idx| x_idx * BYTES_PER_PIXEL)
        .collect();
    let full_y_pxidx: Vec<usize> = (rsz_y1..(rsz_x1 + num_rows_to_copy))
        .map(|y| y as f32 / rsz_max_h)
        .map(|ynorm| (ynorm * inp_max_h).round() as usize)
        .map(|y_idx| y_idx * inp_w * BYTES_PER_PIXEL)
        .collect();

    // Figure out threading setup
    let n_threads = num_threads.unwrap_or({
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, num_rows_to_copy)
    });
    let rows_per_thread = (num_rows_to_copy as f32 / n_threads as f32).ceil() as usize;
    let out_bytes_per_row = out_w * BYTES_PER_PIXEL;
    let bytes_per_thread = out_bytes_per_row * rows_per_thread;

    // Extract segment of output image that we're actually going to change
    // -> We need to grab the full row/column worth of pixels, not just the resized region
    // -> We also pre-compute the boundaries of the segment of each row which is being written to
    let (mut_idx_1, mut_idx_2) = (out_y1 * out_bytes_per_row, out_y2 * out_bytes_per_row);
    let out_mut_segment: &mut [u8] = &mut output_image.as_mut()[mut_idx_1..mut_idx_2];
    let (rowseg_idx_1, rowseg_idx_2) = (out_x1 * BYTES_PER_PIXEL, out_x2 * BYTES_PER_PIXEL);

    // Resize and place pixels into output, using multiple threads
    let in_pixel_data = input_image.as_raw();
    std::thread::scope(|s| {
        // Split as groups of rows
        for (thread_idx, out_row_group_bytes) in out_mut_segment.chunks_mut(bytes_per_thread).enumerate() {
            let ypx_idxs = &full_y_pxidx;
            let xpx_idxs = &full_x_pxidx;
            s.spawn(move || {
                let thread_row_offset = rows_per_thread * thread_idx;

                // Split groups of rows to individual rows
                for (seg_row_idx, out_row_bytes) in out_row_group_bytes.chunks_mut(out_bytes_per_row).enumerate() {
                    let row_idx = seg_row_idx + thread_row_offset;
                    let new_y_pxidx = ypx_idxs[row_idx];

                    // Split rows as per-column RGBA entry
                    let out_row_segment = &mut out_row_bytes[rowseg_idx_1..rowseg_idx_2];
                    for (seg_col_idx, out_col_bytes) in out_row_segment.chunks_mut(BYTES_PER_PIXEL).enumerate() {
                        let rsz_x1 = new_y_pxidx + xpx_idxs[seg_col_idx];
                        let rsz_x2 = rsz_x1 + BYTES_PER_PIXEL;
                        out_col_bytes[0..BYTES_PER_PIXEL].copy_from_slice(&in_pixel_data[rsz_x1..rsz_x2]);
                    }
                }
            });
        }
    });
}
