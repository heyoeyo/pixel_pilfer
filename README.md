# Pixel Pilfer

This project was made to learn & experiment with [Rust](https://rust-lang.org/).

<p align="center">
  <img src="https://github.com/user-attachments/assets/babc6382-f175-4274-8bf7-71e4bdc2e423" width=640 height=360 alt="Pixel pilfer acting on an image of the 'Crab Nebula'">
</p>

It's based on a youtube video about a 'generalized rainbow smoke algorithm' by [Generative Garden](https://www.youtube.com/watch?v=dVQDYne8Bkc).

## Basic Idea

The original algorithm (by [József Fejes](https://www.youtube.com/watch?v=OuvFsB4SLhA)) was developed [in order to construct images](https://codegolf.stackexchange.com/questions/22144/images-with-all-colors) using every value of a set of colors exactly once, where the set was chosen as the [RGB colorspace cube](https://en.wikipedia.org/wiki/RGB_color_spaces). The 'generalized' version uses an image to define the set of colors, and again constructs an image that uses each entry of the set (e.g. each pixel) exactly once.

The version in this repo uses an image like the 'generalized' implementation, but doesn't try to use every pixel as the result tends to look better if this constraint is loosened a bit. There are also options for modifying the input image 'after-the-fact' (e.g. blurring/ color shifting etc.) as well as support for a (mesmerizing!) 'roll' animation.


### The algorithm


The algorithm can be thought of as 'stealing' pixels from the input image in order to build up the output image, hence the name 'pixel pilfer'. Here's a simple step-by-step explanation:

1. Randomly sample a pixel from the input image and a pixel location in the output image
2. Copy (e.g. steal) the input pixel over to the corresponding output location
3. Randomly select an unfilled neighbor around the (now filled) output pixel
4. For the unfilled output pixel, find all it's filled neighbors
5. For all filled neighbors, find the corresponding 'stolen' pixels in the input
6. Randomly select an un-stolen neighbor around one of the stolen input pixels
7. Copy ('steal') the selected input pixel into the selected output pixel (from step 3)
8. Repeat from step 3 until we run out of un-stolen input pixels or fill all output pixels

This is better explained by the aforementioned [youtube video](https://www.youtube.com/watch?v=dVQDYne8Bkc). However one thing the video doesn't mention is that it's very common (e.g. ~30% of the time) for step 6 to fail! This is a result of the randomness of the algorithm, where earlier output pixels may have already 'stolen' all neighboring input pixels. In this case, the algorithm may want to jump to a non-neighbouring input pixel or simply give up and skip stealing for the current output pixel. This can be controlled using the `--no_search` (or `-n`) cli flag and leads to very different styles of output.

If the input and output images have exactly the same number of pixels, then we get the 'generalized rainbow smoke algorithm' described in the video. By default, this implementation _oversizes_ the input slightly, but this can be controlled with the `-s` cli flag.


### Rendering

All rendering is done on the CPU, with many operations (e.g. image resizing) handled manually as a learning exercise. This comes at the cost of very heavy CPU use, especially when fullscreening the display window. On the other hand, given the low-level nature of Rust, there's a surprising amount of room to experiment with optimizations!


## How to run

Building & running requires [cargo](https://rust-lang.org/tools/install/). Assuming it's installed, use:
```bash
cargo run --release
```

This creates an executable file (under `target/release`). The executable can be run in a terminal with a `-h` or `--help` flag to show various launch options, including an option to change the output image sizing.

