use maitake::sync::WaitQueue;
use mycelium_util::{fmt, sync::spin::Mutex};
use pc_keyboard::{layouts, Keyboard};
pub use pc_keyboard::{DecodedKey, KeyCode};

pub struct Ps2Keyboard {
    // TODO(eliza): this should use some kind of broadcast channel that waits
    // for *all* readers to consume each keycode...
    buf: thingbuf::StaticThingBuf<Option<DecodedKey>, 128>,
    /// This doesn't strictly need to be a mutex; we will never spin while
    /// trying to lock it. Instead, we use the mutex to assert that only
    /// ISRs call `handle_scancode`.
    // TODO(eliza): we probably shouldn't assume that the keyboard is always a
    // US 104-key keyboard with scancode set 1, figure out how to
    // detect/configure this...
    kbd: Mutex<Keyboard<layouts::Us104Key, pc_keyboard::ScancodeSet1>>,
    waiters: WaitQueue,
}

static PS2_KEYBOARD: Ps2Keyboard = Ps2Keyboard {
    buf: thingbuf::StaticThingBuf::new(),
    kbd: Mutex::new(
        Keyboard::<layouts::Us104Key, pc_keyboard::ScancodeSet1>::new(
            pc_keyboard::HandleControl::MapLettersToUnicode,
        ),
    ),
    waiters: WaitQueue::new(),
};

pub async fn next_key() -> DecodedKey {
    if let Some(key) = PS2_KEYBOARD.buf.pop() {
        return key.expect("no one should push `None`s to the buffer, this is a bug");
    }

    PS2_KEYBOARD
        .waiters
        .wait()
        .await
        .expect("PS/2 keyboard waiters should never be closed, this is a bug");
    PS2_KEYBOARD
        .buf
        .pop()
        .expect("we just got woken up, there should be a key in the buffer")
        .expect("no one should push `None`s to the buffer, this is a bug")
}

pub(crate) fn handle_scancode(scancode: u8) {
    let mut kbd = PS2_KEYBOARD
        .kbd
        .try_lock()
        .expect("handle_scancode should only be called in an ISR!");
    match kbd.add_byte(scancode) {
        Err(error) => {
            tracing::warn!(
                ?error,
                scancode = fmt::hex(&scancode),
                "error decoding scancode, ignoring it!"
            );
        }
        // state advanced, no character decoded yet
        Ok(None) => {}
        // got a key event
        Ok(Some(event)) => {
            if let Some(decoded_key) = kbd.process_keyevent(event) {
                // got something!
                if let Err(decoded_key) = PS2_KEYBOARD.buf.push(Some(decoded_key)) {
                    tracing::warn!(?decoded_key, "keyboard buffer full, dropping key event!")
                }
                PS2_KEYBOARD.waiters.wake_all();
            }
        }
    };
}
