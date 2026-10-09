use crate::imports::types::DEFAULT_THREAD_COUNT;

pub fn get_elements_per_thread(num_pixels: u32, num_threads: Option<usize>) -> usize {
    /* Helper used to get the largest/evenly sized chunks of data per thread */
    let n_threads = get_num_threads(num_threads, num_pixels);
    return ((num_pixels as f32) / (n_threads as f32)).ceil().max(1.0) as usize;
}

pub fn get_num_threads(num_threads: Option<usize>, max_threads: u32) -> usize {
    /* Helper used to figure out how many threads to use if an explicit value isn't given */
    return num_threads.unwrap_or({
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(DEFAULT_THREAD_COUNT)
            .clamp(1, max_threads as usize)
    });
}
