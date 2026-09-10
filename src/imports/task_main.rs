use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use rand::seq::{IteratorRandom, SliceRandom};
use rand::{random_bool, random_range};

use winit::keyboard::KeyCode::{
    ArrowDown, ArrowLeft, ArrowRight, ArrowUp, Backspace, Delete, KeyB, KeyC, KeyD, KeyF, KeyG, KeyH, KeyK, KeyO, KeyP,
    KeyR, KeyS, KeyZ, Period, Space, Tab,
};
use winit::keyboard::PhysicalKey;
use winit::keyboard::PhysicalKey::Code;

// Custom library imports
use crate::imports;
use image::{ImageError, Rgba, imageops};
use imports::buffer_helpers::{make_resize_buffer, realloc_image_buffer, resize_image};
use imports::cli::CliArgs;
use imports::colormaps::{make_cmap_inferno, make_colormap_lut};
use imports::layout::{DisplayLayout, get_hstack_layout, get_solo_layout};
use imports::postproc::{PostProcessConfig, postprocess_source_image, prepare_sized_image_data};
use imports::state2d::{Visited2D, index_from_xy, random_boundary_index, random_xy_index};
use imports::text::TextDrawer;
use imports::thief_data::{ThiefData, ThiefOverlay};
use imports::types::{RGBAImageU8, UIControl};

// --------------------------------------------------------------------------------------------------------------------

pub struct Task {
    /*
    This struct acts as the interface between the render thread and our actual display data/logic.
    All public function are only ever called by the render thread.
    */
    data: WorkData,
    render: RenderData,

    enable_animation: bool,
    request_one_rerender: bool,
    request_one_state_step: bool,
    pause_on_reset: bool,
    ui_focused: UIControl,

    source_image_name: String,
    date_id: String,
    base_save_path: Option<PathBuf>,
    draw_count: u32,
}

impl Task {
    pub fn new(args: CliArgs) -> Self {
        // Figure out output sizing (user can provide 1 or 2 numbers)
        let out_size = &args.output_wh;
        let num_sizes = out_size.len();
        let target_out_wh;
        if num_sizes == 1 {
            target_out_wh = (out_size[0] as usize, out_size[0] as usize);
        } else if num_sizes == 2 {
            target_out_wh = (out_size[0] as usize, out_size[1] as usize);
        } else {
            panic!("Unexpected output sizing! Should be 1 or 2 entries, got: {}", num_sizes);
        }

        // Set up initial samples (if present)
        let mut init_out_xy: Option<(f32, f32)> = None;
        if let Some(init_out_xy_arg) = &args.output_xy {
            init_out_xy = Some((init_out_xy_arg[0], init_out_xy_arg[1]));
        }
        let mut init_src_xy: Option<(f32, f32)> = None;
        if let Some(init_src_xy_arg) = &args.source_xy {
            init_src_xy = Some((init_src_xy_arg[0], init_src_xy_arg[1]));
        }

        // Initialize rendering data
        let init_roll_speed = (args.roll_speed_xy[0], args.roll_speed_xy[1]);
        let loaded_img = make_default_image(800, 800);
        let posproc_cfg = PostProcessConfig::from_cli(&args);
        let mut render_data = RenderData::new(posproc_cfg, init_roll_speed);
        let src_wh = render_data.store_image(loaded_img, target_out_wh);

        // Compute number of pixels to 'steal' to achieve target running time
        let num_out_pixels = target_out_wh.0 * target_out_wh.1;
        let num_px_steal = num_out_pixels as f32 / (args.framerate * args.target_steal_time_sec);

        // Initialize working dataset
        let mut work_data = WorkData::new(
            target_out_wh,
            init_out_xy,
            init_src_xy,
            !args.disable_full_search,
            num_px_steal.max(1.0).round() as usize,
        );
        work_data.setup(src_wh);

        // Set up task data
        let pause_on_reset = args.pause_on_reset;
        let mut new_task = Self {
            data: work_data,
            render: render_data,
            enable_animation: !pause_on_reset,
            request_one_rerender: true,
            request_one_state_step: false,
            pause_on_reset: pause_on_reset,
            ui_focused: UIControl::Roll,
            date_id: Self::get_new_date_id(),
            base_save_path: std::env::current_dir().ok().map(|p| p.join("saved_images")),
            source_image_name: "default_pattern".to_string(),
            draw_count: 0,
        };

        // Check if we got an image
        if let Some(img_path) = &args.image_path {
            new_task.load_image(img_path);
        }
        return new_task;
    }

    fn get_new_date_id() -> String {
        /* Helper used to get a timestamp to use for saving (used to distinguish resets) */
        let start_of_2026 = std::time::UNIX_EPOCH + Duration::from_secs(1767225600);
        let date_id = SystemTime::now().duration_since(start_of_2026).unwrap().as_secs();
        return format!("{}", date_id);
    }

    pub fn load_image(&mut self, image_path: &PathBuf) {
        /*
        This is a special function called by the window thread in response to user loaded images
        (e.g. from a keypress)...
        This is a bit of a messy implementation/confusing control flow!
        -> Would prefer to handle this as a keypress event, but...
        -> The file selection prompt requires a reference to the window, so easier to have window thread handle it
        */

        match image::open(&image_path) {
            Ok(img) => {
                // Some feedback/record keeping
                println!("Loaded image: {:?}", image_path);

                // Cursed conversion from image path to a name for saving
                let src_name_as_str = image_path
                    .file_stem() // Ex: '/path/to/image.jpeg' -> 'image'
                    .unwrap_or(OsStr::new("noname")) // Handle missing file name (shouldn't happen)
                    .to_string_lossy()
                    .to_owned()
                    .to_string();

                // Update stored source data
                let src_wh = self.render.store_image(img.to_rgba8(), self.data.out_wh);
                self.data.setup(src_wh);
                self.source_image_name = src_name_as_str;

                // Trigger re-renders
                self.request_one_rerender = true;
                self.enable_animation = true;
            }
            Err(load_error) => match load_error {
                ImageError::Unsupported(e) => {
                    println!("Error loading image!");
                    println!("Unsupported file type: {}", e.format_hint());
                }
                other => {
                    println!("An unknown error occured while trying to load the image file!");
                    println!("{:?}", other);
                }
            },
        }
    }

    pub fn draw(&mut self, display_buffer: &mut RGBAImageU8, target_frame_duration: Duration) {
        /* Main draw update function, called by render thread */
        if (self.request_one_state_step || self.enable_animation) && !self.data.is_done {
            let is_done = self.data.step_state(target_frame_duration);
            if is_done && !self.render.has_roll_speed() {
                self.enable_animation = false;
            }
        }
        self.render.render_display_images(display_buffer, &self.data.thief_data);
        self.request_one_state_step = false;
        self.draw_count += 1;
    }

    pub fn need_redraw(&mut self) -> bool {
        /* Function used to indicate (to render thread) whether we can/need to redraw output */
        let need_redraw = self.enable_animation | self.request_one_rerender | self.request_one_state_step;
        self.request_one_rerender = false;
        return need_redraw;
    }

    pub fn force_pause(&mut self) {
        /* Force task to stop if needed (e.g. for use by render thread) */
        self.enable_animation = false;
    }

    pub fn handle_key_events(&mut self, keypress: PhysicalKey) {
        match keypress {
            // Toggle playback
            Code(Space) => {
                self.enable_animation = !self.enable_animation;
            }

            // Step animation forward 1 frame
            Code(Period) => {
                self.enable_animation = false;
                self.request_one_state_step = true;
            }

            // Reset state
            Code(Backspace | Delete) => {
                self.date_id = Self::get_new_date_id();
                self.render.roll_offset_xy = (0.0, 0.0);
                self.data.reset();
                self.render.clear();
                self.enable_animation = !self.pause_on_reset;
                self.request_one_rerender = true;
                self.draw_count = 0;
            }

            // Cycle display layout
            Code(Tab) => {
                match self.render.layout_state {
                    DisplayLayout::HStack => self.render.layout_state = DisplayLayout::NoText,
                    DisplayLayout::NoText => self.render.layout_state = DisplayLayout::Solo,
                    DisplayLayout::Solo => self.render.layout_state = DisplayLayout::HStack,
                }
                self.request_one_rerender = true;
            }

            // Toggle pause on reset
            Code(KeyP) => {
                self.pause_on_reset = !self.pause_on_reset;
                println!("Pause on reset: {}", self.pause_on_reset);
            }

            // Toggle overlay display
            Code(KeyO) => {
                self.render.toggle_overlay(None);
                self.request_one_rerender = true;
            }

            // Reset roll state
            Code(KeyK) => {
                self.render.roll_offset_xy = (0.0, 0.0);
                self.enable_animation = false;
                self.request_one_rerender = true;
            }

            // Reset roll state
            Code(KeyF) => {
                self.render.enable_render_timer = !self.render.enable_render_timer;
                self.request_one_rerender = true;
            }

            // Reset roll state
            Code(KeyG) => {
                self.render.post_proc_cfg.is_grayscale = !self.render.post_proc_cfg.is_grayscale;
                self.render.apply_post_processing();
                self.request_one_rerender = true;
            }

            // Reset/zero-out current control value
            Code(KeyZ) => {
                let mut need_post_proc = true;
                match self.ui_focused {
                    UIControl::Blur => self.render.post_proc_cfg.blur = 0,
                    UIControl::Contrast => self.render.post_proc_cfg.contrast = 0.0,
                    UIControl::Dirt => self.render.post_proc_cfg.dirt = 0,
                    UIControl::Hue => self.render.post_proc_cfg.hue_rotate = 0,
                    UIControl::Roll => {
                        self.render.roll_speed_xy = (0.0, 0.0);
                        self.enable_animation = false;
                        need_post_proc = false;
                    }
                }
                self.request_one_rerender = true;
                if need_post_proc {
                    self.render.apply_post_processing();
                }
            }

            Code(KeyB | KeyC | KeyD | KeyH | KeyR) => {
                let prev_focused = self.ui_focused.clone();
                match keypress {
                    Code(KeyB) => self.ui_focused = UIControl::Blur,
                    Code(KeyC) => self.ui_focused = UIControl::Contrast,
                    Code(KeyD) => self.ui_focused = UIControl::Dirt,
                    Code(KeyH) => self.ui_focused = UIControl::Hue,
                    Code(KeyR) => self.ui_focused = UIControl::Roll,
                    _ => {}
                }

                // Some feedback about control changes
                let is_focus_changed = prev_focused != self.ui_focused;
                if is_focus_changed {
                    self.render.set_text_display(None, Some(&self.ui_focused));
                    self.request_one_rerender = true;
                }
            }

            // Adjust focused value
            Code(ArrowUp | ArrowDown | ArrowRight | ArrowLeft) => {
                // For convenience
                let is_positive = matches!(keypress, Code(ArrowUp | ArrowRight));
                let arrow_dir: i32 = if is_positive { 1 } else { -1 };
                let mut needs_post_proc = true;

                // Adjust only the focused control
                match self.ui_focused {
                    UIControl::Blur => {
                        self.render.post_proc_cfg.blur = self
                            .render
                            .post_proc_cfg
                            .blur
                            .saturating_add_signed(5 * arrow_dir as i8);
                    }
                    UIControl::Contrast => {
                        let new_contrast = self.render.post_proc_cfg.contrast + arrow_dir as f32;
                        self.render.post_proc_cfg.contrast = new_contrast.clamp(-100.0, 100.0);
                    }
                    UIControl::Dirt => {
                        self.render.post_proc_cfg.dirt = self
                            .render
                            .post_proc_cfg
                            .dirt
                            .saturating_add_signed(5 * arrow_dir as i8);
                    }
                    UIControl::Hue => {
                        let new_value = self.render.post_proc_cfg.hue_rotate + (5 * arrow_dir);
                        self.render.post_proc_cfg.hue_rotate = (new_value + 360) % 360; // Force 0-to-360 range
                        self.render.post_proc_cfg.is_grayscale = false;
                    }
                    UIControl::Roll => {
                        let arrow_dir_f32 = arrow_dir as f32;
                        if keypress == Code(ArrowLeft) || keypress == Code(ArrowRight) {
                            self.render.change_roll_speed(Some(0.5 * arrow_dir_f32), None);
                        }
                        if keypress == Code(ArrowDown) || keypress == Code(ArrowUp) {
                            self.render.change_roll_speed(None, Some(0.5 * arrow_dir_f32));
                        }
                        self.enable_animation |= self.render.has_roll_speed() && !self.pause_on_reset;
                        needs_post_proc = false;
                    }
                }

                // Trigger render updates
                self.request_one_rerender = true;
                if needs_post_proc {
                    self.render.apply_post_processing();
                }
            }

            // Save data
            Code(KeyS) => {
                if let Some(base_folder) = &self.base_save_path {
                    // Build save folder: base_path / image_name / date_id
                    let save_folder = base_folder.join(&self.source_image_name).join(&self.date_id);
                    if !save_folder.exists() {
                        let res = std::fs::create_dir_all(&save_folder);
                        if res.is_err() {
                            println!("Error, unable to create save directory!");
                        }
                    }

                    // Save the image
                    let save_name = format!("{}_{}.png", self.data.thief_data.get_iter_count(), self.draw_count);
                    let save_buffer = &self.render.disp_out_buffer;
                    if let Some(save_file_path) = save_folder.join(save_name).to_str() {
                        let res = save_buffer.save(&save_file_path);
                        if res.is_ok() {
                            println!("Saved output: {}", save_file_path);
                        } else {
                            println!("Error, unable to save image: {}", res.unwrap_err());
                        }
                    }
                } else {
                    println!("Cannot save! Unable to determine save folder...");
                }
            }

            _ => {}
        };
    }

    pub fn print_key_controls(&self) {
        println!(
            "
--------------------------
Pixel Pilfer key controls:
--------------------------

Load image: enter
Reset output state: backspace, delete
Toggle animation: space
Toggle view layout: tab
Toggle pause-on-reset: p
Toggle sample overlay: o
Toggle render timer: f
Toggle grayscale: g
Reset roll offsets: k
Step one frame: period
Save image: s

*** Use arrow keys to adjust setting
Adjust blur: b
Adjust contrast: c
Adjust dirty blur: d
Adjust hue: h
Adjust roll xy: r
Reset current setting: z
        "
        );
    }
}

// . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .

pub struct WorkData {
    /*
       This is the main working data on the entire program
       ALl pixel 'stealing' is handled & stored by this struct
    */
    pub src_wh: (usize, usize),
    pub out_wh: (usize, usize),
    pub is_done: bool,
    enable_full_search: bool,
    num_steal_per_iter: usize,
    out_visited: Visited2D,
    src_visited: Visited2D,
    bfs_visited: Visited2D,
    uv_out_nbs: Vec<usize>,
    pub thief_data: ThiefData,
    init_out_sample: Option<(f32, f32)>,
    init_src_sample: Option<(f32, f32)>,
}

impl WorkData {
    pub fn new(
        output_wh: (usize, usize),
        initial_output_sample_xy_norm: Option<(f32, f32)>,
        initial_source_sample_xy_norm: Option<(f32, f32)>,
        enable_full_search: bool,
        initial_num_pixel_steal: usize,
    ) -> Self {
        let init_src_wh = (0, 0);
        Self {
            out_wh: output_wh,
            src_wh: init_src_wh,
            enable_full_search: enable_full_search,
            num_steal_per_iter: initial_num_pixel_steal.max(1),
            is_done: false,
            out_visited: Visited2D::new(output_wh),
            src_visited: Visited2D::new(init_src_wh),
            bfs_visited: Visited2D::new(init_src_wh),
            uv_out_nbs: Vec::new(),
            thief_data: ThiefData::new(output_wh, init_src_wh),
            init_out_sample: initial_output_sample_xy_norm,
            init_src_sample: initial_source_sample_xy_norm,
        }
    }

    pub fn setup(&mut self, source_wh: (usize, usize)) -> &mut Self {
        // Resize visited states, if we see a change in the shared data
        let (curr_src_w, curr_src_h) = self.thief_data.order_map.dimensions();
        if curr_src_w != source_wh.0 || curr_src_h != source_wh.1 {
            self.src_wh = (source_wh.0, source_wh.1);
            self.src_visited.resize(source_wh.0, source_wh.1);
            self.bfs_visited.resize(source_wh.0, source_wh.1);
        }
        self.thief_data.resize(self.src_wh, self.out_wh);

        // Make sure we clear any existing state
        self.reset();

        return self;
    }

    pub fn reset(&mut self) -> &mut Self {
        // Make sure storage is clean (in case we're re-running this)
        self.thief_data.clear();
        self.out_visited.clear();
        self.src_visited.clear();
        self.bfs_visited.clear();
        self.uv_out_nbs.clear();
        self.is_done = false;

        // Pick sample points
        // -> We re-use user provided sample points (if given) otherwise randomize on every reset
        let mut init_out_sample: usize = random_boundary_index(self.out_wh.0, self.out_wh.1);
        if let Some(out_sample_xy) = self.init_out_sample {
            let out_x_px = out_sample_xy.0.clamp(0.0, 1.0) * (self.out_wh.0.saturating_sub(1) as f32);
            let out_y_px = out_sample_xy.1.clamp(0.0, 1.0) * (self.out_wh.1.saturating_sub(1) as f32);
            init_out_sample = index_from_xy(out_x_px.round() as usize, out_y_px.round() as usize, self.out_wh.0);
        }
        let mut init_src_sample: usize = random_xy_index(self.src_wh.0, self.src_wh.1);
        if let Some(src_sample_xy) = self.init_src_sample {
            let src_x_px = src_sample_xy.0.clamp(0.0, 1.0) * (self.src_wh.0.saturating_sub(1) as f32);
            let src_y_px = src_sample_xy.1.clamp(0.0, 1.0) * (self.src_wh.1.saturating_sub(1) as f32);
            init_src_sample = index_from_xy(src_x_px.round() as usize, src_y_px.round() as usize, self.src_wh.0);
        }

        // Record initial sampling locations
        self.thief_data.record_mapping(init_out_sample, init_src_sample);
        self.src_visited.set_visited(init_src_sample);
        self.out_visited.set_visited(init_out_sample);
        self.uv_out_nbs
            .append(&mut self.out_visited.get_neighbours_unvisited(init_out_sample));

        return self;
    }

    pub fn step_state(&mut self, max_duration: Duration) -> bool {
        /*
        Main working function of the entire program!
        This is where pixel stealing actually occurs
        Returns: is_done
        */

        // Iterate over all pixels if we're not given a max count
        let timer = Instant::now();
        for _ in 0..self.num_steal_per_iter {
            // Stop if we ever run for too long (ensures we update the display regularly)
            if timer.elapsed() > max_duration {
                break;
            }

            // Stop if we have no more pixels to draw
            if self.uv_out_nbs.len() == 0 {
                self.is_done = true;
                break;
            }

            // Randomly choose an unvisited output point from neighbors
            let rand_uv_onb_idx = random_range(0..self.uv_out_nbs.len());
            let next_out_sample = self.uv_out_nbs.swap_remove(rand_uv_onb_idx);

            // Get all visited src points associated with chosen output (we want nearest src neighbour as next point)
            let visited_out_nbs = self.out_visited.get_neighbours_visited(next_out_sample);
            let visited_src_pts: Vec<usize> = visited_out_nbs
                .iter()
                .map(|pt| self.thief_data.read(*pt).unwrap())
                .collect();
            debug_assert!(visited_out_nbs.len() == 0, "No visited nbs around out-sample point!");
            debug_assert!(visited_src_pts.len() == 0, "No visited source points!");

            // Try to sample the next source point for coloring in the sampled output point
            let mut try_next_src_sample = sample_source_point_nb(&visited_src_pts, &mut self.src_visited);
            if try_next_src_sample.is_none() && self.enable_full_search {
                try_next_src_sample =
                    sample_source_point_bfs(visited_src_pts, &mut self.src_visited, &mut self.bfs_visited);
            }

            // Record src->out mapping and mark pixels as visited
            if let Some(next_src_sample) = try_next_src_sample {
                self.thief_data.record_mapping(next_out_sample, next_src_sample);
                self.out_visited.set_visited(next_out_sample);
                self.src_visited.set_visited(next_src_sample);
                self.uv_out_nbs
                    .append(&mut self.out_visited.get_neighbours_unsearched(next_out_sample));
            };
        }

        return self.is_done;
    }
}

// . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .

pub struct RenderData {
    /*
    Holds resources for managing render updates.
    Holds the original image, a resized (for pilfer) copy and a post-processed copy.
    It also holds full-res copies of the final rendered image data.
    */
    loaded_src: RGBAImageU8,      // Holds original loaded image data
    base_src_buffer: RGBAImageU8, // Holds clean image scaled to target resolution
    post_src_buffer: RGBAImageU8, // Holds image with post-processing applyied (main working data)
    post_proc_cfg: PostProcessConfig,

    disp_out_buffer: RGBAImageU8, // Buffer used for rendering output image
    disp_src_buffer: RGBAImageU8, // Buffer used for rendering source image (with overlay)

    resize_buffer: RGBAImageU8, // Buffer used to temporarily store resized images

    thief_olay: ThiefOverlay,
    enable_olay: bool,

    txtdraw: TextDrawer,
    focused_ctrl: UIControl,
    image_name: String,
    roll_speed_xy: (f32, f32),
    roll_offset_xy: (f32, f32),
    layout_state: DisplayLayout,
    enable_render_timer: bool,
}

impl RenderData {
    pub fn new(post_process_config: PostProcessConfig, initial_roll_speed: (f32, f32)) -> Self {
        // Make dummy starter data
        let empty_img = RGBAImageU8::new(0, 0);

        Self {
            loaded_src: empty_img.clone(),
            base_src_buffer: empty_img.clone(),
            post_src_buffer: empty_img.clone(),
            post_proc_cfg: post_process_config,
            disp_out_buffer: empty_img.clone(),
            disp_src_buffer: empty_img.clone(),
            resize_buffer: make_resize_buffer(),
            thief_olay: ThiefOverlay::new(make_cmap_inferno()),
            enable_olay: true,
            txtdraw: TextDrawer::new_regular(24.0),
            focused_ctrl: UIControl::Roll,
            image_name: "Default Pattern".to_string(),
            roll_speed_xy: initial_roll_speed,
            roll_offset_xy: (0.0, 0.0),
            layout_state: DisplayLayout::HStack,
            enable_render_timer: false,
        }
    }

    pub fn store_image(&mut self, loaded_image: RGBAImageU8, output_wh: (usize, usize)) -> (usize, usize) {
        // Record loaded image & size to match output pixel count
        self.loaded_src = loaded_image;
        prepare_sized_image_data(
            &mut self.base_src_buffer,
            &self.loaded_src,
            output_wh,
            self.post_proc_cfg.relative_src_size,
        );
        self.post_src_buffer = self.base_src_buffer.clone();
        self.disp_src_buffer = self.post_src_buffer.clone();
        self.roll_offset_xy = (0.0, 0.0);

        // Resize output display if needed
        let (out_w, out_h) = (output_wh.0 as u32, output_wh.1 as u32);
        if self.disp_out_buffer.width() != out_w || self.disp_out_buffer.height() != out_h {
            realloc_image_buffer(&mut self.disp_out_buffer, (out_w, out_h));
        }

        // Reset render state
        let src_wh = (
            self.base_src_buffer.width() as usize,
            self.base_src_buffer.height() as usize,
        );
        self.thief_olay.resize(src_wh);
        self.clear();
        self.apply_post_processing();

        return src_wh;
    }

    pub fn clear(&mut self) {
        self.thief_olay.clear();
        self.disp_out_buffer.fill(0);
    }

    pub fn set_text_display(&mut self, image_name: Option<&str>, active_control: Option<&UIControl>) {
        if let Some(name) = image_name {
            self.image_name = name.to_string();
        }
        if let Some(ctrl) = active_control {
            self.focused_ctrl = ctrl.clone();
        }
    }

    pub fn apply_post_processing(&mut self) -> &mut Self {
        self.post_src_buffer = postprocess_source_image(&self.base_src_buffer, &self.post_proc_cfg);
        self.disp_src_buffer = self.post_src_buffer.clone();
        return self;
    }

    pub fn change_roll_speed(&mut self, new_x: Option<f32>, new_y: Option<f32>) -> &mut Self {
        if let Some(x_speed_px) = new_x {
            self.roll_speed_xy.0 += x_speed_px;
        }
        if let Some(y_speed_px) = new_y {
            self.roll_speed_xy.1 += y_speed_px;
        }
        return self;
    }

    pub fn has_roll_speed(&self) -> bool {
        return self.roll_speed_xy.0 != 0.0 || self.roll_speed_xy.1 != 0.0;
    }

    pub fn toggle_overlay(&mut self, new_state: Option<bool>) -> &mut Self {
        self.enable_olay = new_state.unwrap_or(!self.enable_olay);
        return self;
    }

    pub fn render_display_images(&mut self, display_buffer: &mut RGBAImageU8, thief_data: &ThiefData) {
        // Render output image on it's own (at full resolution)
        let (src_w, src_h) = self.disp_src_buffer.dimensions();
        if self.roll_speed_xy.0 != 0.0 || self.roll_speed_xy.1 != 0.0 {
            self.roll_offset_xy.0 = (self.roll_offset_xy.0 + self.roll_speed_xy.0) % src_w as f32;
            self.roll_offset_xy.1 = (self.roll_offset_xy.1 + self.roll_speed_xy.1) % src_h as f32;
        }
        thief_data.render_result(
            &mut self.disp_out_buffer,
            &self.post_src_buffer,
            self.roll_offset_xy,
            None,
        );

        // Render indicator over original image showing where pixels where taken from
        self.disp_src_buffer = self.post_src_buffer.clone();
        if self.enable_olay {
            self.thief_olay
                .draw_overlay(&mut self.disp_src_buffer, &thief_data.order_map);
        }

        // Clear background
        display_buffer.fill(0);

        // Handle scaling (e.g. to fit window) image outputs for display
        let render_timer = Instant::now();
        let pad_anchor = Some((0.5, 0.5));
        match self.layout_state {
            // In this case we show output & input side-by-side
            DisplayLayout::HStack | DisplayLayout::NoText => {
                // For clarity, define a small space for text outputs
                let txt_pad = 5;
                let show_text = self.layout_state != DisplayLayout::NoText;
                let avail_wh = if show_text {
                    let txt_space = self.txtdraw.get_font_size() as u32 + txt_pad;
                    (display_buffer.width(), display_buffer.height() - txt_space)
                } else {
                    display_buffer.dimensions()
                };

                // Figure out h-stack sizing/placement & draw into output
                let hstack_imgs = [&self.disp_out_buffer, &self.disp_src_buffer];
                let (outer_hstack, overlay_items) = get_hstack_layout(
                    avail_wh,
                    self.disp_out_buffer.dimensions(),
                    self.disp_src_buffer.dimensions(),
                    8,
                    pad_anchor,
                );

                // Scale each image and blit into output
                for (olay, img) in std::iter::zip(overlay_items, hstack_imgs) {
                    resize_image(&img, &mut self.resize_buffer, olay.wh, None);
                    imageops::overlay(display_buffer, &self.resize_buffer, olay.x, olay.y);
                }

                if show_text {
                    // Decide what control text to show
                    let disp_txt = match &self.focused_ctrl {
                        UIControl::Blur => format!("Blur: {}", self.post_proc_cfg.blur),
                        UIControl::Contrast => format!("Contrast: {}", self.post_proc_cfg.contrast),
                        UIControl::Dirt => format!("Dirt: {}", self.post_proc_cfg.dirt),
                        UIControl::Hue => format!("Hue: {}", self.post_proc_cfg.hue_rotate),
                        UIControl::Roll => format!("Roll speed: {}, {}", self.roll_speed_xy.0, self.roll_speed_xy.1),
                    };
                    let txt_y = outer_hstack.wh.1 + outer_hstack.y.max(0) as u32 + txt_pad;
                    self.txtdraw.xy_px(display_buffer, &disp_txt, (5, txt_y));

                    // Draw text to indicate source image sizing (helpful for roll settings)
                    let size_txt = &format!(
                        "Source WH: {} x {}",
                        self.post_src_buffer.width(),
                        self.post_src_buffer.height()
                    );
                    let (size_w, _, _) = self.txtdraw.get_text_size(size_txt);
                    let size_x = (display_buffer.width() - txt_pad).saturating_sub(size_w as u32);
                    self.txtdraw.xy_px(display_buffer, size_txt, (size_x, txt_y));
                }
            }

            // In this case we only show the output (no need for original image + sample overlay)
            DisplayLayout::Solo => {
                let olay = get_solo_layout(
                    display_buffer.dimensions(),
                    self.disp_out_buffer.dimensions(),
                    pad_anchor,
                );
                resize_image(&self.disp_out_buffer, &mut self.resize_buffer, olay.wh, None);
                imageops::overlay(display_buffer, &self.resize_buffer, olay.x, olay.y);
            }
        }

        // Draw top-left indicator showing time needed to draw frame
        if self.enable_render_timer {
            let time_ms = render_timer.elapsed().as_micros();
            let render_time_str = format!("{} us", time_ms);
            self.txtdraw.xy_px(display_buffer, &render_time_str, (5, 5));
        }
    }
}

fn sample_source_point_nb(visited_src_points: &Vec<usize>, src_visited: &mut Visited2D) -> Option<usize> {
    /*
    Function used to pick the next source sample point (if possible).
    This works by listing out the nearest (unvisited) neighbors of all
    of the given (visited) source points, and then picking one randomly.

    This can (and often does!) fail if there are no unvisited neighbors.
    */

    // Get unvisited neighbour of each visited src point (if any)
    let uv_src_nbs_iter = visited_src_points
        .iter()
        .flat_map(|src_pt| src_visited.get_neighbours_unvisited(*src_pt));

    // Randomly pick one of the unvisited neighbors as the sample point
    // -> This can fail as it's fairly common that we have no neighbors remaining!
    return uv_src_nbs_iter.choose(&mut rand::rng());
}

fn sample_source_point_bfs(
    visited_src_points: Vec<usize>,
    src_visited: &mut Visited2D,
    bfs_search_visited: &mut Visited2D,
) -> Option<usize> {
    /*
    Function used to search for a nearest unvisited source sample point.
    This is only meant to be called if we can't find an immediate neighbor
    point to use.

    This works by searching each of the neighbors around the visited
    source points in a breadth-first-search manner. If no unvisited
    point is found among the neighbors, the we repeat by checking the
    neighbors of the neighbors etc.

    This search can fail when there are no more source points to sample
    from, though this should only happen if we 'undersize' the source
    image (e.g. it has fewer pixels than the output).

    It would be nice to accelerate this search, maybe by keeping a count
    of how many points are left to sample from (e.g. on a low-res grid)...?
    Would allow us to speed up search when there are no nearby points left.
    */

    // Set up new search map for finding closest unvisited src nb
    bfs_search_visited.clear();
    for pt in visited_src_points.iter() {
        bfs_search_visited.set_visited(*pt);
    }

    // Repeatedly do breadth-first-search of neighbouring source points until we find an unvisited point
    let mut points_to_check: Vec<usize> = visited_src_points;
    while points_to_check.len() > 0 {
        let mut new_points_to_check: Vec<usize> = Vec::with_capacity(points_to_check.len() * 4);
        for pt in points_to_check.iter().copied() {
            let mut bfs_nbs = bfs_search_visited.get_neighbours_unsearched(pt);
            bfs_nbs.shuffle(&mut rand::rng());
            for search_nb in bfs_nbs {
                // Stop if we find an unvisited src point (considered 'close' to influence points)
                if !src_visited.is_visited(search_nb) {
                    return Some(search_nb);
                }

                // If point isn't unvisited, mark as searched and record for next round of checks
                new_points_to_check.push(search_nb);
            }
        }

        // Update points to check with newly found points
        points_to_check = new_points_to_check;
    }

    return None;
}

fn make_default_image(image_w: u32, image_h: u32) -> RGBAImageU8 {
    /* Helper used to make a default image in case nothing is loaded */

    // Build simple gradient to use for making a default pattern
    let cmap = vec![
        Rgba([5, 10, 25, 255]),
        Rgba([0, 50, 50, 255]),
        Rgba([10, 110, 95, 255]),
        Rgba([40, 175, 100, 255]),
        Rgba([90, 235, 75, 255]),
        Rgba([130, 255, 90, 255]),
        Rgba([0, 75, 90, 255]),
    ];
    let cmap = make_colormap_lut(&cmap);
    let max_cmap_idx = cmap.len() - 1;
    let max_cmap_idx_f32 = max_cmap_idx as f32;

    // Draw image, pixel-by-pixel, with twirling effect
    // -> This is based on the 'twirl-node' from the Unity game engine
    let img_wh = (image_w / 2, image_h / 2);
    let (x_cen, y_cen) = (random_range(0.25..0.75), random_range(0.25..0.75));
    let effect_strength = random_range(2.0..8.0) * (2 * random_bool(0.5) as i32 - 1) as f32;
    let mut out_img = RGBAImageU8::new(img_wh.0 as u32, img_wh.1 as u32);
    for (x, y, pixel) in out_img.enumerate_pixels_mut() {
        let x_norm = x as f32 / img_wh.0 as f32;
        let y_norm = y as f32 / img_wh.1 as f32;
        let (dx, dy) = (x_norm - x_cen, y_norm - y_cen);
        let dist = (dx * dx + dy * dy).sqrt();
        let angle = effect_strength * dist;
        let mut twirl = angle.cos() * dx - angle.sin() * dy + x_cen;

        // Reflect values outside 0.0-1.0 range
        if twirl < 0.0 {
            twirl = twirl.abs();
        }
        if twirl > 1.0 {
            twirl = 2.0 - twirl;
        }

        let y_idx = (twirl * max_cmap_idx_f32).round() as usize;
        *pixel = cmap[y_idx.clamp(0, max_cmap_idx)];
    }

    return out_img;
}
