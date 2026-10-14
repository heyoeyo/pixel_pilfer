
# The algorithm


The algorithm can be thought of as 'stealing' pixels from the input image in order to build up the output image, hence the name 'pixel pilfer'. Here's an attempt at a simple step-by-step explanation:

#### Setup
1. Randomly sample a pixel from the input image and a pixel location in the output image
2. Copy (e.g. steal) the selected input pixel over to the selected output location

#### Loop
1. Randomly select an unfilled neighbor around any of the filled output pixels
2. For the unfilled output pixel, find all it's filled neighbors
3. For all filled neighbors, find the corresponding 'stolen' pixels in the input
4. Randomly select an un-stolen neighbor around one of the stolen input pixels
5. Copy ('steal') the selected input pixel into the selected output pixel (from step 1)
6. Repeat until we run out of un-stolen input pixels or fill all output pixels

This is better explained by the [youtube video](https://www.youtube.com/watch?v=dVQDYne8Bkc) where the idea comes from. The implementation from the video also assumes that the input and output images have the same number of pixels, so that every input pixel will appear exactly once in the output image. By default, the implementation in this repo _oversizes_ the input slightly, but this can be controlled with the `-s` cli flag.

One thing the video doesn't mention is that it's very common (e.g. ~30% of the time) for step 4 to fail! This is a result of the randomness of the algorithm, where earlier output pixels may have already 'stolen' all neighboring input pixels. The choice for how to deal with this situation has a surprisingly big impact on the final result. The original implementation seems to expand the search to find the nearest un-stolen pixel (e.g. no longer an immediate neighbor), which tends to produce radially symmetric patterns. An alternative approach is to skip the pixel, which often leads to more interesting patterns as it forces the sampling to re-route around the (unfilled) pixel. This can be enabled using the `--no_search` cli flag (or by pressing the `n` key while running).


## Code Structure

This implementation handles both the windowing and rendering logic, with as much of the work being done manually (on CPU only) as a learning exercise.

At the top-most level, the code is split into a window thread & render thread, with some shared data (e.g. image buffers) between them. Most of the algorithm implementation is found in the [task_main.rs](https://github.com/heyoeyo/pixel_pilfer/blob/main/src/imports/task_main.rs) file, more specifically the `step_state` function.

All rendering is done on the CPU which can be very heavy when fullscreening the display window and runs much slower than what a GPU could manage (especially rendering the mapped output image). However, the rendering steps (e.g. resizing, blurring, roll-offset sampling etc.) do provide a lot of opportunities to experiment with low-level optimizations for anyone interested in that.