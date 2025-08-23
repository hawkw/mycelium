use maitake::sync::WaitQueue;
use mycelium_util::{fmt, sync::InitOnce};

pub struct SerialInput {
    // TODO(eliza): this should use some kind of broadcast channel that waits
    // for *all* readers to consume each byte...
    buf: thingbuf::StaticThingBuf<Option<u8>, 128>,
    waiters: WaitQueue,
}

impl SerialInput {
    pub fn new() -> Self {
        Self {
            buf: thingbuf::StaticThingBuf::new(),
            waiters: WaitQueue::new(),
        }
    }
}

pub static SERIAL_INPUTS: InitOnce<alloc::vec::Vec<SerialInput>> = InitOnce::uninitialized();

/*
static PS2_KEYBOARD: Ps2Keyboard = Ps2Keyboard {
    buf: thingbuf::StaticThingBuf::new(),
    kbd: Mutex::new(
        Keyboard::<layouts::Us104Key, pc_keyboard::ScancodeSet1>::new(
            pc_keyboard::HandleControl::MapLettersToUnicode,
        ),
    ),
    waiters: WaitQueue::new(),
};
*/

impl SerialInput {
    pub async fn next_byte(&self) -> u8 {
        if let Some(byte) = self.buf.pop() {
            return byte.expect("buffer contains somes");
        }

        self.waiters.wait().await.expect("serial waiters should never be closed FOR NOW");
        self.buf.pop().expect("just got woken up wtf").expect("buffer contains somes")
    }

    pub fn handle_input(&self, byte: u8) {
        self.buf.push(Some(byte));
        self.waiters.wake_all();
    }
}
