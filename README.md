# Pixel Pilfer

This project is an attempt to implement the 'Generalized Rainbow Smoke Algorithm' (from [Generative Garden](https://www.youtube.com/watch?v=dVQDYne8Bkc) on youtube), and to help learn [Rust](https://rust-lang.org/).

<p align="center">
  <img src="https://github.com/user-attachments/assets/babc6382-f175-4274-8bf7-71e4bdc2e423" width=320 height=180 alt="Pixel pilfer acting on an image of the 'Crab Nebula'">
</p>

The rendering is handled 'manually' as often as possible and done entirely on the CPU.

Building & running requires [cargo](https://rust-lang.org/tools/install/). Assuming it's installed, use:
```bash
cargo run --release
```

This creates an executable file (under `target/release`). The executable can be run in a terminal with a `-h` or `--help` flag to show various launch options, including an option to change the output image sizing.

