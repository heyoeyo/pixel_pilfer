use std::sync::mpsc::{Iter, Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

// Custom imports
use crate::imports::types::{ChannelMessage, RGBAImageU8};

// --------------------------------------------------------------------------------------------------------------------
// Structs

pub struct SwapBuffer {
    /*
    Special buffer meant to be shared between a 'writer' and 'reader' thread.
    Each thread has it's own local buffer, which it works with directly.
    When finished, the 'local' and 'swap' buffer trade pointers. This allows
    each thread to operate mostly independent of the other, with only a brief
    mutex lock to trade pointers. The 'is_ready' flag is set to true whenever
    the writer swaps new data in, and false when the reader swaps the data out.
    */
    pub image: RGBAImageU8,
    is_ready: bool,
}

impl SwapBuffer {
    pub fn new() -> Self {
        Self {
            image: RGBAImageU8::new(0, 0),
            is_ready: true,
        }
    }
}

// . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .

pub struct SharedStateReader {
    /*
    This is the 'reader' component of the shared reader/writer buffers.
    This struct is meant to hold the buffer that is read out for display (by the window thread)
    */
    pub local_buffer: RGBAImageU8,
    pub swap_buffer: Arc<Mutex<SwapBuffer>>,
    pub channel_tx: Sender<ChannelMessage>,
}

impl SharedStateReader {
    pub fn read_swap_buffer(&mut self) -> bool {
        let mut is_updated = false;
        if let Ok(mut swap_data) = self.swap_buffer.try_lock() {
            if swap_data.is_ready {
                std::mem::swap(&mut swap_data.image, &mut self.local_buffer);
                swap_data.is_ready = false;
                is_updated = true;
            }
        } else {
            println!("******** UNABLE TO ACQUIRE SWAPBUFFER LOCK ON READ! ********");
        }
        return is_updated;
    }

    pub fn send_to_writer(&self, message: ChannelMessage) {
        self.channel_tx
            .send(message)
            .expect("Unable to send message to writer! Cannot continue");
    }
}

// . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .

pub struct SharedStateWriter {
    /*
    This is the 'writer' component of the shared reader/writer buffers.
    This struct is meant to hold the buffer that is drawn into (by the render thread)
    */
    pub local_buffer: RGBAImageU8,
    pub swap_buffer: Arc<Mutex<SwapBuffer>>,
    pub channel_rx: Receiver<ChannelMessage>,
}

impl SharedStateWriter {
    pub fn write_swap_buffer(&mut self) -> bool {
        let mut is_success = false;
        if let Ok(mut swap_data) = self.swap_buffer.lock() {
            std::mem::swap(&mut swap_data.image, &mut self.local_buffer);
            swap_data.is_ready = true;
            is_success = true;
        }
        return is_success;
    }

    pub fn read_messages_blocking(&self) -> Iter<'_, ChannelMessage> {
        return self.channel_rx.iter();
    }

    pub fn read_messages_nonblocking(&self) -> Vec<ChannelMessage> {
        return self.channel_rx.try_iter().collect();
    }
}

// --------------------------------------------------------------------------------------------------------------------
// Functions

pub fn setup_shared_thread_state() -> (SharedStateWriter, SharedStateReader) {
    let reader_swap = Arc::new(Mutex::new(SwapBuffer::new()));
    let writer_swap = Arc::clone(&reader_swap);
    let (ch_tx, ch_rx): (Sender<ChannelMessage>, Receiver<ChannelMessage>) = channel();

    let reader_state = SharedStateReader {
        local_buffer: RGBAImageU8::new(0, 0),
        swap_buffer: reader_swap,
        channel_tx: ch_tx,
    };

    let writer_state = SharedStateWriter {
        local_buffer: RGBAImageU8::new(0, 0),
        swap_buffer: writer_swap,
        channel_rx: ch_rx,
    };

    return (writer_state, reader_state);
}
