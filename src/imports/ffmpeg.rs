use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Instant;

use crate::imports;
use imports::buffer_helpers::resize_and_overlay;
use imports::types::RGBAImageU8;

// --------------------------------------------------------------------------------------------------------------------

#[cfg(target_os = "windows")]
const DEFAULT_FFMPEG_EXEC: &str = "ffmpeg.exe";

#[cfg(not(target_os = "windows"))]
const DEFAULT_FFMPEG_EXEC: &str = "ffmpeg";

// --------------------------------------------------------------------------------------------------------------------
// Structs

pub struct FFMpegWriter {
    exec_path: PathBuf,
    wh_str: String,
    fps_str: String,
    needs_scaling: bool,
    render_buffer: RGBAImageU8,
    proc: Option<Child>,
    stdin: Option<ChildStdin>,
    pub total_frames: u32,
    curr_fidx: u32,
    capture_timer: Instant,
    save_idx: u32,
    save_folder: Option<PathBuf>,
}

impl FFMpegWriter {
    pub fn new(
        save_folder_path: Option<PathBuf>,
        ffmpeg_exec: Option<PathBuf>,
        frame_wh: (usize, usize),
        render_wh: Option<(usize, usize)>,
        total_frames: u32,
        framerate: f32,
    ) -> Self {
        // Handle case where recording size is different from frame size
        let mut needs_scaling = false;
        let mut record_wh = frame_wh;
        let mut render_buffer = RGBAImageU8::new(0, 0);
        if let Some(render_wh) = render_wh {
            record_wh = render_wh;
            needs_scaling = frame_wh.0 != render_wh.0 || frame_wh.1 != render_wh.1;
            if needs_scaling {
                render_buffer = RGBAImageU8::new(record_wh.0 as u32, record_wh.1 as u32);
            }
        }

        return Self {
            exec_path: ffmpeg_exec.unwrap_or(PathBuf::from(DEFAULT_FFMPEG_EXEC)),
            wh_str: format!("{}x{}", record_wh.0, record_wh.1),
            fps_str: format!("{}", framerate),
            render_buffer: render_buffer,
            needs_scaling: needs_scaling,
            proc: None,
            stdin: None,
            total_frames: total_frames,
            curr_fidx: 0,
            save_idx: 0,
            capture_timer: Instant::now(),
            save_folder: save_folder_path,
        };
    }

    pub fn get_recording_index(&self) -> Option<(u32, u32)> {
        /* Will return None when not recording, otherwise returns: (current_frame_index, total_frames) */
        return if self.proc.is_some() {
            Some((self.curr_fidx, self.total_frames))
        } else {
            None
        };
    }

    pub fn begin_capture(&mut self, file_name: &str) {
        // Clean up existing captures
        if self.proc.is_some() {
            self.end_capture();
        }

        // Bail if we don't have a saving path
        if self.save_folder.is_none() {
            eprintln!("Cannot save video, invalid pathing...");
            return;
        }
        let save_folder = self.save_folder.clone().unwrap();
        if !save_folder.exists() {
            let res = std::fs::create_dir_all(&save_folder);
            if res.is_err() {
                println!("Error, unable to create save directory!");
            }
            return;
        }

        // Deal up ffmpeg cli args
        let save_name = if self.save_idx == 0 {
            format!("{}.mp4", file_name)
        } else {
            format!("{}_{}.mp4", file_name, self.save_idx)
        };
        let save_path = save_folder.join(save_name);
        let fixed_args = [
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-s",
            &self.wh_str,
            "-r",
            &self.fps_str,
            "-i",
            "-",
        ];
        let dynamic_args = [
            "-vcodec",
            "libx264",
            "-crf",
            "20",
            "-pix_fmt",
            "yuv444p",
            save_path.to_str().unwrap(),
        ];
        let full_args: Vec<&str> = fixed_args.into_iter().chain(dynamic_args).collect();

        // Some feedback
        let exec_str = self.exec_path.to_str().unwrap_or("exec");
        println!();
        println!("Opening FFMpeg for recording with command:");
        println!("  {} {}", exec_str, dynamic_args.join(" "));

        // Launch ffmpeg as child process, with ability to write to input directly
        let Some(mut proc) = Command::new(&self.exec_path)
            .args(full_args)
            .stdin(Stdio::piped())
            .stderr(Stdio::null()) // Suppress ffmpeg terminal output
            .spawn()
            .ok()
        else {
            eprintln!("Error! Unable to spawn ffmpeg to record video...");
            return;
        };

        // Store for re-use
        let stdin = proc.stdin.take();
        self.proc = Some(proc);
        self.stdin = stdin;
        self.curr_fidx = 0;
        self.save_idx += 1;
        self.capture_timer = Instant::now();
    }

    pub fn write_frame(&mut self, frame: &RGBAImageU8) -> bool {
        /* Write frame data direct to ffmpeg process. Returns: is_finished */

        if let Some(ffmpeg_stdin) = &mut self.stdin {
            let frame_bytes = if self.needs_scaling {
                let render_wh = self.render_buffer.dimensions();
                resize_and_overlay(&mut self.render_buffer, &frame, (0, 0), render_wh, None);
                self.render_buffer.as_raw()
            } else {
                frame.as_raw()
            };
            ffmpeg_stdin
                .write_all(frame_bytes)
                .expect("Error writing frame data! Bad ffmpeg command?");
            self.curr_fidx += 1;

            // Clean up when we've recorded all frames
            if self.curr_fidx >= self.total_frames {
                self.end_capture();
            }
        } else {
            eprintln!("Error! FFMpeg is not open, no frames being written");
        }

        return self.proc.is_none();
    }

    pub fn end_capture(&mut self) {
        // Tell ffmpeg we're done writing
        drop(self.stdin.take());

        // Wait for ffmpeg to finish
        let mut proc = &mut self.proc.take();
        if let Some(proc) = &mut proc {
            let status = proc.wait().unwrap();
            if !status.success() {
                eprintln!("Error! FFMpeg was unable to exit cleanly: {}", status);
            }
            println!(
                "Ended ffmpeg capture! Took {:.1} mins",
                self.capture_timer.elapsed().as_secs() as f32 / 60.0,
            );
        }

        // Reset frame indexing
        self.curr_fidx = 0;
    }
}
