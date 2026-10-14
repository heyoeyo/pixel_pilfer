# Pixel Pilfer

This project was made to learn & experiment with [Rust](https://rust-lang.org/).

<p align="center">
  <img src="https://github.com/user-attachments/assets/babc6382-f175-4274-8bf7-71e4bdc2e423" width=640 height=360 style="image-rendering: pixelated;" alt="Pixel pilfer acting on an image of the 'Crab Nebula'">
</p>

It's based on a youtube video about a 'generalized rainbow smoke algorithm' by [Generative Garden](https://www.youtube.com/watch?v=dVQDYne8Bkc).

## Basic Idea

The original algorithm (by [József Fejes](https://www.youtube.com/watch?v=OuvFsB4SLhA)) was developed [in order to construct images](https://codegolf.stackexchange.com/questions/22144/images-with-all-colors) using every value of a set of colors exactly once, where the set was chosen as the [RGB colorspace cube](https://en.wikipedia.org/wiki/RGB_color_spaces). The 'generalized' version uses an image to define the set of colors, and again constructs an image that uses each entry of the set (e.g. each pixel) exactly once.

The version in this repo uses an image like the 'generalized' implementation, but doesn't strictly try to use every pixel once. More information about the implementation can be found in the [src folder readme](https://github.com/heyoeyo/pixel_pilfer/tree/main/src).

## How to run

Building & running requires [cargo](https://rust-lang.org/tools/install/). Assuming it's installed, use:
```bash
cargo run --release
```

This creates an executable file (under `target/release`). The executable can be run in a terminal with a `-h` or `--help` flag to show various launch options, including an option to change the output image sizing.

