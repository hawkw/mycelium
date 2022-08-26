use maitake::wait::WaitCell;
use mycelium_util::sync::Lazy;
use thingbuf::mpsc;

const KEYBOARD_PORT: u16 = 0x60;

type Scancode = u8;

type ScancodeRx = mpsc::StaticReceiver<Scancode>;
type ScancodeTx = mpsc::StaticSender<Scancode>;
static Q: StaticChannel<Scancode, 256> = StaticChannel::new();
static SCANCODE_QUEUE: Lazy<(Option<mpsc::StaticReceiver<) = {}
pub(super) fn isr() {}
