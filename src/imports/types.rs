use image::{ImageBuffer, Rgba};
use std::path::PathBuf;
use winit::keyboard::PhysicalKey;

// --------------------------------------------------------------------------------------------------------------------

pub type Colormap = [Rgba<u8>; 1024];

pub enum ChannelMessage {
    /* Used to move data between window-to->render thread */
    NewWindow((u32, u32), (u32, u32)), // (display_wh, max_wh)
    KeyPress(PhysicalKey),
    Resize((u32, u32)),
    LoadedImage(PathBuf),
    Pause,
}

pub enum TimerRedrawEvent {
    /* Used by render thread to trigger re-draws */
    Redraw,
}

pub type RGBAImageU8 = ImageBuffer<Rgba<u8>, Vec<u8>>;

#[derive(Debug, Clone, PartialEq)]
pub enum UIControl {
    Blur,
    Contrast,
    Dirt,
    Hue,
    Roll,
}
