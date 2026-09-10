use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;
use winit::event_loop::EventLoopProxy;

use crate::imports;
use imports::buffer_helpers::{init_buffer_size, realloc_image_buffer};
use imports::shared_thread_data::SharedStateWriter;
use imports::task_main::Task;
use imports::types::{ChannelMessage, TimerRedrawEvent};

// --------------------------------------------------------------------------------------------------------------------
// Structs

pub struct RenderThread {
    /*
    The render thread is responsible for updating rendered display data.
    It's basically a wrapper around the main task logic, but helps handle data sharing with the window thread
    */
    shared_state: SharedStateWriter,
    event_loop: EventLoopProxy<TimerRedrawEvent>,
    target_frame_duration: Duration,
    resize_wh: (u32, u32),
    task: Task,
}

impl RenderThread {
    pub fn new(
        shared_state: SharedStateWriter,
        event_loop: EventLoopProxy<TimerRedrawEvent>,
        framerate: f32,
        task: Task,
    ) -> Self {
        Self {
            shared_state,
            event_loop,
            target_frame_duration: Duration::from_secs_f32(1.0 / framerate),
            resize_wh: (2, 2),
            task: task,
        }
    }

    fn handle_shared_message(&mut self, message: ChannelMessage) {
        /* Helper used to read/respond to channel messages from the window thread */
        match message {
            ChannelMessage::NewWindow(display_wh, max_wh) => {
                // Over-allocate to max dispaly size on startup, so resizing is smooth
                init_buffer_size(&mut self.shared_state.local_buffer, display_wh, max_wh);
                self.resize_wh = display_wh;
            }
            ChannelMessage::Resize(new_wh) => {
                self.resize_wh = new_wh;
            }
            ChannelMessage::LoadedImage(file_path) => {
                self.task.load_image(&file_path);
            }
            ChannelMessage::KeyPress(key) => {
                self.task.handle_key_events(key);
            }
            ChannelMessage::Pause => {
                self.task.force_pause();
            }
        }
    }

    pub fn spawn_thread(mut self) -> JoinHandle<()> {
        let thread = std::thread::Builder::new().name("render-thread".to_string());
        return thread
            .spawn(move || {
                // Wait for window to be created before doing any work
                for msg in self.shared_state.read_messages_blocking() {
                    match msg {
                        ChannelMessage::NewWindow(_, _) => {
                            self.handle_shared_message(msg);
                            break;
                        }
                        _ => { /* Ignore other events until window is created */ }
                    }
                }

                // Let user know about key commands
                self.task.print_key_controls();

                // Run rendering task as infinite loop
                let mut timer = Instant::now();
                loop {
                    // Listen for messages that might alter render state
                    for msg in self.shared_state.read_messages_nonblocking() {
                        self.handle_shared_message(msg);
                    }

                    // This is used to force the task to re-draw on window size changes
                    // -> We need to check repeatedly to make sure all buffers end up resized!
                    let (curr_w, curr_h) = self.shared_state.local_buffer.dimensions();
                    let (targ_w, targ_h) = self.resize_wh;
                    let need_resize = (curr_w != targ_w) || (curr_h != targ_h);
                    if need_resize {
                        realloc_image_buffer(&mut self.shared_state.local_buffer, self.resize_wh);
                    }

                    // Have task render a new frame & perform buffer swap with window thread
                    if self.task.need_redraw() | need_resize {
                        self.task
                            .draw(&mut self.shared_state.local_buffer, self.target_frame_duration);

                        // Perform buffer swap to provide window thread with new display data
                        self.shared_state.write_swap_buffer();
                        let _ = self.event_loop.send_event(TimerRedrawEvent::Redraw);
                    }

                    // Delay to create target framerate
                    let sleep_time = self.target_frame_duration.saturating_sub(timer.elapsed());
                    std::thread::sleep(sleep_time);
                    timer = Instant::now();
                }
            })
            .expect("Unable to spawn render thread");
    }
}
