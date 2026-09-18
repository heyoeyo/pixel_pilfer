use std::path::PathBuf;

use clap::Parser;

// --------------------------------------------------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(version, about, next_line_help = true)]
pub struct CliArgs {
    #[arg(
        short = 'i',
        help = "Path to input image (will use a default pattern if not provided)"
    )]
    pub image_path: Option<PathBuf>,

    #[arg(
        short = 'o',
        num_args = 1..=2,
        default_values_t = vec![800, 450],
        value_names = ["OUTPUT_W", "OUTPUT_H (optional)"],
        help = "The size of the generated output image in pixels",
    )]
    pub output_wh: Vec<u32>,

    #[arg(
        short = 's',
        default_value_t = 1.1,
        help = "Set relative pixel count of source-to-output image. Values less than 1 lead to incomplete output images"
    )]
    pub source_rel_scale: f32,

    #[arg(
        short = 'y',
        num_args=2,
        value_names = ["DISPLAY_W", "DISPLAY_H"],
        help = "The size of the initial display window in pixels",
    )]
    pub display_wh: Option<Vec<u32>>,

    #[arg(short = 'b', default_value_t = 0, help = "Set blur strength on startup (0 to 255)")]
    pub blur: u8,

    #[arg(
        short = 'd',
        default_value_t = 0,
        help = "Set dirty blur strength on startup (0 to 255)"
    )]
    pub dirt: u8,

    #[arg(short = 'u', default_value_t = 0, help = "Set hue rotation on startup (0 to 360)")]
    pub hue: i32,

    #[arg(
        short = 'c',
        default_value_t = 0.0,
        help = "Set contrast adjustment on startup (-100 to +100)",
        allow_negative_numbers = true
    )]
    pub contrast: f32,

    #[arg(
    short = 'r',
    long ="roll_xy",
    num_args = 2,
    default_values_t = vec![0.0, 0.0],
    value_names = ["X-Speed", "Y-Speed"],
    help = "Set initial roll speed",
    )]
    pub roll_speed_xy: Vec<f32>,

    #[arg(short = 'f', long = "fps", default_value_t = 60.0, help = "Set target framerate")]
    pub framerate: f32,

    #[arg(
        short = 'x',
        long ="out_xy",
        num_args = 2,
        value_names = ["X", "Y"],
        help = "Set initial output sample point (ex: 0.5 0.5)",
    )]
    pub output_xy: Option<Vec<f32>>,

    #[arg(
        short = 'z',
        long ="src_xy",
        num_args = 2,
        value_names = ["X", "Y"],
        help = "Set initial source sample point (ex: 0.25 0.75)",
    )]
    pub source_xy: Option<Vec<f32>>,

    #[arg(short = 'p', long = "pause", help = "Pause the algorithm on startup & on resets")]
    pub pause_on_reset: bool,

    #[arg(
        short = 'n',
        long = "no_search",
        help = "Disables full-search mode. This will run faster, but the resulting image will have holes"
    )]
    pub disable_full_search: bool,

    #[arg(
        short = 'g',
        long = "galvanized",
        help = "If enabled, output points are preferentially sampled near previous sample points (can be slow)"
    )]
    pub enable_galvanized_mode: bool,

    #[arg(
        long = "debug_profiling",
        help = "Compute full mapping without render updates and print timing. Used for debugging/profiling"
    )]
    pub debug_profiling: bool,

    #[arg(
        short = 't',
        long = "time_sec",
        default_value_t = 8.0,
        help = "Target amount of time to finish stealing all pixels (limited by CPU power)"
    )]
    pub target_steal_time_sec: f32,
}

// --------------------------------------------------------------------------------------------------------------------
// Functions

pub fn parse_args() -> CliArgs {
    /* Simple helper. Avoids caller file needing to import clap modules */
    return CliArgs::parse();
}
