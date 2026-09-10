use crate::imports::types;
use types::RGBAImageU8;

// --------------------------------------------------------------------------------------------------------------------
// Functions

pub fn make_resize_buffer() -> RGBAImageU8 {
    /* Helper to make (reasonable) max-sized buffer to be re-used for resizing */
    RGBAImageU8::new(3840, 2160)
}

pub fn realloc_image_buffer(image_buffer: &mut RGBAImageU8, new_wh: (u32, u32)) -> bool {
    /*
    Function used to resize an image buffer without allocating memory, if possible (e.g. when downsizing).
    This is similar to resizing vectors, but works on image buffers.
    Returns:
        needed_memory_allocation
    */

    // Handle re-allocating (if we need a bigger buffer) or re-using memory for smaller buffers
    let current_capacity = image_buffer.as_raw().capacity();
    let required_capacity = (new_wh.0 * new_wh.1 * 4) as usize;
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

pub fn resize_image(
    input_image: &RGBAImageU8,
    output_image: &mut RGBAImageU8,
    new_wh: (u32, u32),
    num_threads: Option<usize>,
) {
    /*
    Faster (compared to image crate) nearest-neighbor resize implementation.

    Note this requires providing the output image, which this function will
    also resize (in terms of memory allocation) to match the provided new_wh.
    */

    // Resize underlying image vector and get total working pixel count for spreading threaded workload
    realloc_image_buffer(output_image, new_wh);
    let (out_w, out_h) = (output_image.width() as usize, output_image.height() as usize);
    let num_out_pixels = out_w * out_h;

    // Figure out how many threads to use
    let n_threads = num_threads.unwrap_or({
        std::thread::available_parallelism()
            .map(|n| n.get() / 2)
            .unwrap_or(4)
            .clamp(1, num_out_pixels)
    });

    // Have threads working on entire rows!
    // -> This lets us compute resizing slightly more efficiently by re-using indexing data
    let rows_per_thread = (out_h as f32 / n_threads as f32).ceil() as usize;
    let bytes_per_row = out_w * 4;
    let bytes_per_thread = bytes_per_row * rows_per_thread;

    // For convenience
    let (inp_w, inp_h) = (input_image.width() as usize, input_image.height() as usize);
    let (inp_max_w, inp_max_h) = ((inp_w - 1) as f32, (inp_h - 1) as f32);
    let (out_max_w, out_max_h) = ((out_w - 1) as f32, (out_h - 1) as f32);

    // Pre-compute resized xy-indexing with 4x byte & dimension scaling
    let full_x_pxidx: Vec<usize> = (0..out_w)
        .map(|x| x as f32 / out_max_w)
        .map(|xnorm| (xnorm * inp_max_w).round() as usize)
        .map(|x_idx| x_idx * 4)
        .collect();
    let full_y_pxidx: Vec<usize> = (0..out_h)
        .map(|y| y as f32 / out_max_h)
        .map(|ynorm| (ynorm * inp_max_h).round() as usize)
        .map(|y_idx| y_idx * inp_w * 4)
        .collect();

    // Split image into groups of rows, handled by separate threads
    let in_pixel_data = input_image.as_raw();
    std::thread::scope(|s| {
        // Split as groups of rows
        for (thread_idx, thread_img_bytes) in output_image.chunks_mut(bytes_per_thread).enumerate() {
            let ypx_idxs = &full_y_pxidx;
            let xpx_idxs = &full_x_pxidx;
            s.spawn(move || {
                let row_offset = rows_per_thread * thread_idx;

                // Split groups of rows to individual rows
                for (rel_row_idx, row_bytes) in thread_img_bytes.chunks_mut(bytes_per_row).enumerate() {
                    let row_idx = row_offset + rel_row_idx;
                    let new_y_pxidx = ypx_idxs[row_idx];

                    // Split rows as per-column RGBA entry (e.g. 4 bytes)
                    for (col_idx, col_bytes) in row_bytes.chunks_mut(4).enumerate() {
                        let new_px_idx = new_y_pxidx + xpx_idxs[col_idx];
                        col_bytes[0..4].copy_from_slice(&in_pixel_data[new_px_idx..new_px_idx + 4]);
                    }
                }
            });
        }
    });
}
