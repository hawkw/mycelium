use maitake::sync::WaitQueue;
use mycelium_util::sync::InitOnce;

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

impl Default for SerialInput {
    fn default() -> Self {
        Self::new()
    }
}

pub static SERIAL_INPUTS: InitOnce<alloc::vec::Vec<SerialInput>> = InitOnce::uninitialized();

impl SerialInput {
    pub async fn next_byte(&self) -> u8 {
        if let Some(byte) = self.buf.pop() {
            return byte.expect("buffer contains somes");
        }

        self.waiters
            .wait()
            .await
            .expect("serial waiters should never be closed FOR NOW");
        self.buf
            .pop()
            .expect("just got woken up wtf")
            .expect("buffer contains somes")
    }

    pub fn handle_input(&self, byte: u8) {
        if let Err(byte) = self.buf.push(Some(byte)) {
            // TODO(ixi): We were trying to handle serial console input but we're so backed up that
            // we don't have space in the buffer to accept a byte. We should probably mask the
            // interrupt until space in the buffer is available, but instead for now we'll eat the
            // byte and drop it.
            tracing::warn!(?byte, "serial buffer full, dropping byte!")
        } else {
            // We actually enqueued a byte, let everyone know.
            self.waiters.wake_all();
        }
    }
}
