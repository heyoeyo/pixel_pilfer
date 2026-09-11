use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use image::imageops;
use rfd::FileDialog;
use softbuffer::{Buffer as SBuffer, Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

// Custom library imports
use crate::imports;
use imports::buffer_helpers::init_buffer_size;
use imports::shared_thread_data::SharedStateReader;
use imports::types::{ChannelMessage, RGBAImageU8, TimerRedrawEvent};

// --------------------------------------------------------------------------------------------------------------------

pub struct WindowThread {
    /*
    The window thread is responsible for handling window events, as well as updating the display drawn to screen.
    It's not meant to be doing direct pixel manipulation/rendering however! This is handled by the render thread
    */
    window: Option<Arc<Window>>,
    surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    init_display_wh: Option<(u32, u32)>,
    curr_display_wh: (u32, u32),
    shared_state: SharedStateReader,
    interpolation: imageops::FilterType,
    prev_folder_path: Option<PathBuf>,
}

impl WindowThread {
    pub fn new(
        shared_state: SharedStateReader,
        initial_image_path: Option<PathBuf>,
        initial_display_wh: Option<(u32, u32)>,
    ) -> Self {
        // Special handling of initial image path. We want the window to remember the parent folder
        // -> This way, if user loads more images, the file picker starts in the same folder as the loaded file
        let mut prev_folder_path = None;
        if let Some(img_path) = initial_image_path {
            if let Some(parent_path) = img_path.parent() {
                prev_folder_path = Some(parent_path.to_path_buf());
            }
        }

        return Self {
            window: None,
            surface: None,
            init_display_wh: initial_display_wh,
            curr_display_wh: (8, 8),
            shared_state: shared_state,
            interpolation: imageops::FilterType::Nearest,
            prev_folder_path: prev_folder_path,
        };
    }

    fn handle_window_creation(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            // Figure out monitor sizing to decide initial window size
            let monitor_info = event_loop
                .primary_monitor()
                .or_else(|| event_loop.available_monitors().next())
                .expect("No monitor found!?");
            let monitor_size = monitor_info.size();
            let disp_wh = self.init_display_wh.unwrap_or((
                (monitor_size.width as f32 * 0.5).round() as u32,
                (monitor_size.height as f32 * 0.5).round() as u32,
            ));

            // Resize our buffer to the max size to reduce need for re-allocation on resizing
            let max_wh = (monitor_size.width, monitor_size.height);
            init_buffer_size(&mut self.shared_state.local_buffer, disp_wh, max_wh);

            // Resize swap as well, since we should have guaranteed access to modify it
            let mut swap_data = self
                .shared_state
                .swap_buffer
                .try_lock()
                .expect("Unable to access locked swap buffer! This shouldn't happen on window creation");
            init_buffer_size(&mut swap_data.image, disp_wh, max_wh);

            // Notify background thread about window size for proper rendering
            self.shared_state
                .send_to_writer(ChannelMessage::NewWindow(disp_wh, max_wh));

            // Configure window
            #[allow(unused_mut)] // Avoids warning on non-linux systems
            let mut window_attributes = Window::default_attributes()
                .with_title("Pixel Pilfer")
                .with_resizable(true)
                .with_min_inner_size(PhysicalSize::new(64, 64))
                .with_max_inner_size(PhysicalSize::new(max_wh.0, max_wh.1))
                .with_inner_size(PhysicalSize::new(disp_wh.0, disp_wh.1));
            #[cfg(target_os = "linux")]
            {
                // Set app_id on wayland
                use winit::platform::wayland::WindowAttributesExtWayland;
                window_attributes = window_attributes.with_name("pixel_pilfer", "");
            }

            // Set up rendering resources
            let window = event_loop.create_window(window_attributes).unwrap();
            let ref_window = Arc::new(window);
            let context = Context::new(ref_window.clone()).unwrap();
            let mut surface = Surface::new(&context, ref_window.clone()).unwrap();

            // Make sure surface is correctly sized on startup
            let nonzero_default = NonZeroU32::new(8).unwrap();
            let w_nonzero = NonZeroU32::new(disp_wh.0).unwrap_or(nonzero_default);
            let h_nonzero = NonZeroU32::new(disp_wh.1).unwrap_or(nonzero_default);
            surface.resize(w_nonzero, h_nonzero).unwrap();

            // Store for re-use
            self.window = Some(ref_window);
            self.surface = Some(surface);
            self.curr_display_wh = disp_wh;
        }
    }

    fn handle_resize_event(&mut self, new_size: PhysicalSize<u32>) {
        // Bail on bad state
        let new_wh = (new_size.width, new_size.height);
        let (Some(window), Some(surface), Some(width_nz), Some(height_nz)) = (
            &self.window,
            &mut self.surface,
            NonZeroU32::new(new_wh.0),
            NonZeroU32::new(new_wh.1),
        ) else {
            return;
        };

        // Update display sizing & notify writer thread
        if let Ok(_) = surface.resize(width_nz, height_nz) {
            window.request_redraw();
            self.curr_display_wh = new_wh;
            self.shared_state.send_to_writer(ChannelMessage::Resize(new_wh));
        };
    }

    fn handle_redraw_event(&mut self) {
        // Bail if we have nothing to draw to
        let Some(surface) = &mut self.surface else {
            return;
        };

        // Check for new display data from the writer thread
        let is_new_frame = self.shared_state.read_swap_buffer();
        let needs_rerender = if is_new_frame {
            true
        } else {
            // We should re-render on size changes (which can occur without getting new frames)
            // -> If we don't do this, the window can't be resized when the animation is paused!
            let (w_local, h_local) = self.shared_state.local_buffer.dimensions();
            let is_resized = w_local != self.curr_display_wh.0 || h_local != self.curr_display_wh.1;
            is_resized
        };

        // Update if writer gives us new data or data doesn't match display size
        // -> Mismatch size occurs right after resize, when data from writer is stale
        // -> Size mismatch can occur while paused, which is why we need to force re-render on resize
        if needs_rerender {
            if let Ok(mut surf_buffer) = surface.buffer_mut() {
                draw_image_to_buffer(&self.shared_state.local_buffer, &mut surf_buffer, self.interpolation);
                surf_buffer.present().unwrap();
            }
        }
    }
}

// --------------------------------------------------------------------------------------------------------------------
// %% Winit logic

// This code connects our custom 'reader' to winit logic
impl ApplicationHandler<TimerRedrawEvent> for WindowThread {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.handle_window_creation(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, window_event: WindowEvent) {
        match window_event {
            // Handle closing on keypress
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        state: ElementState::Pressed,
                        repeat: false,
                        ..
                    },
                ..
            } => {
                event_loop.exit();
            }

            // Special keypress listener for opening a file picker for loading a new image
            // -> We do this here (instead of handling in writer thread) because we need a
            // reference to the window, but don't want to have to pass it for every keypress...
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::Enter),
                        state: ElementState::Pressed,
                        repeat: false,
                        ..
                    },
                ..
            } => {
                // Prevent animation from continuing while user picks file
                self.shared_state.send_to_writer(ChannelMessage::Pause);

                // Build file prompt & try to re-use previous folder path, if user loads multiple times
                let mut file_dialog = FileDialog::new()
                    .set_parent(&self.window.clone().expect("No parent window! This shouldn't happen..."))
                    .set_title("Select an image")
                    .add_filter("Images", &["jpg", "png", "webp", "*"]);
                if let Some(parent_dir) = &self.prev_folder_path {
                    file_dialog = file_dialog.set_directory(parent_dir);
                }

                // If user selects a file, give path to other thread to load/reset state
                let selected_file: Option<PathBuf> = file_dialog.pick_file();
                if let Some(path) = selected_file {
                    // Record parent path for use as starting point on future file dialogs
                    if let Some(parent_path) = &path.parent() {
                        self.prev_folder_path = Some(parent_path.to_path_buf());
                    }
                    self.shared_state.send_to_writer(ChannelMessage::LoadedImage(path));
                }
            }

            // Handle non-closing key events
            WindowEvent::KeyboardInput {
                event:
                    key_event @ KeyEvent {
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                // Pass keypresses to writer thread
                self.shared_state
                    .send_to_writer(ChannelMessage::KeyPress(key_event.physical_key));
            }
            WindowEvent::Resized(physical_size) => {
                self.handle_resize_event(physical_size);
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw_event();
            }
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            _ => (),
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, user_event: TimerRedrawEvent) {
        /*
        Custom user event that just triggers draw updates.
        This is used by the 'timer thread' to regularly trigger re-draws at a specified frame rate.
        It helps avoid messy 'about-to-wait' winit logic
        */
        match user_event {
            TimerRedrawEvent::Redraw => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
    }
}

// --------------------------------------------------------------------------------------------------------------------
// Functions

fn draw_image_to_buffer(
    image: &RGBAImageU8,
    buffer: &mut SBuffer<'_, Arc<Window>, Arc<Window>>,
    interpolation: imageops::FilterType,
) {
    /* Helper used to move image data (from image crate) into softbuffer */

    // Resize to match buffer if needed
    // -> This is only expected to happen on first frame after resize (before shared buffers are updated)
    let (w_buf, h_buf) = (buffer.width().get(), buffer.height().get());
    let img_bytes = if w_buf != image.width() || h_buf != image.height() {
        &imageops::resize(image, w_buf, h_buf, interpolation)
    } else {
        image
    };

    // Copy image into softbuffer
    // -> Image is ordered as RRGGBBAA,
    // -> Buffer order is XXRRGGBB (the alpha channel is ignored!)
    img_bytes
        .chunks_exact(4)
        .zip(buffer.iter_mut())
        .for_each(|(rgba, pixel)| {
            *pixel = ((rgba[0] as u32) << 16) | ((rgba[1] as u32) << 8) | (rgba[2] as u32);
        });
}
