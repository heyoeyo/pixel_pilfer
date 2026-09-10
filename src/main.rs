use winit::event_loop::{ControlFlow, EventLoop};

// Custom library imports
mod imports;
use imports::cli::parse_args;
use imports::render_thread::RenderThread;
use imports::shared_thread_data::setup_shared_thread_state;
use imports::task_main::Task;
use imports::types::TimerRedrawEvent;
use imports::window_thread::WindowThread;

// --------------------------------------------------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get cli args
    let args = parse_args();
    let fps = args.framerate;
    let init_img_path = args.image_path.clone();
    let init_disp_wh: Option<(u32, u32)>;
    if let Some(init_disp_wh_vec) = &args.display_wh {
        init_disp_wh = Some((init_disp_wh_vec[0], init_disp_wh_vec[1]));
    } else {
        init_disp_wh = None;
    }

    // Set up main task data (this is where the bulk of the app logic lives)
    let task = Task::new(args);

    // Set up window event loop for winit
    let event_loop = EventLoop::<TimerRedrawEvent>::with_user_event().build().unwrap();
    let evt_proxy = event_loop.create_proxy();

    // Initialize thread data
    let (shared_writer_state, shared_reader_state) = setup_shared_thread_state();
    let render_thread = RenderThread::new(shared_writer_state, evt_proxy, fps, task);
    let mut window_thread = WindowThread::new(shared_reader_state, init_img_path, init_disp_wh);

    // Run window/event loop on main thread (required by OS) & data/rendering on background thread
    render_thread.spawn_thread();
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut window_thread)?;

    return Ok(());
}
