//! Key decoding and screen-mode helpers shared by full-screen builtins
//! (`vi`, `less`, `more`).

use super::{Tty, TtyEvent};

/// A decoded keypress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Ctrl(u8),
}

/// Raw mode (+ optional alternate screen) for a full-screen program's
/// lifetime, restored on drop (including when the execution is cancelled).
pub(crate) struct ScreenGuard<'a> {
    tty: &'a Tty,
    alternate: bool,
}

impl<'a> ScreenGuard<'a> {
    pub(crate) fn enter(tty: &'a Tty, alternate: bool) -> Self {
        tty.set_raw(true);
        if alternate {
            tty.write(b"\x1b[?1049h\x1b[H\x1b[2J");
        }
        Self { tty, alternate }
    }
}

impl Drop for ScreenGuard<'_> {
    fn drop(&mut self) {
        self.tty.write(b"\x1b[?25h");
        if self.alternate {
            self.tty.write(b"\x1b[?1049l");
        }
        self.tty.set_raw(false);
    }
}

/// Read one key, decoding UTF-8 and CSI/SS3 escape sequences.
/// `Some(Err(()))` means the terminal was resized; `None` that it closed.
pub(crate) async fn read_key(tty: &Tty) -> Option<std::result::Result<Key, ()>> {
    let b = match tty.read_event().await {
        TtyEvent::Byte(b) => b,
        TtyEvent::Resize => return Some(Err(())),
        TtyEvent::Closed => return None,
    };
    let key = match b {
        0x1b => match tty.peek_byte() {
            Some(b'[') | Some(b'O') => {
                tty.try_read_byte();
                let mut params = Vec::new();
                let mut final_byte = 0;
                for _ in 0..16 {
                    match tty.try_read_byte() {
                        Some(b) if (0x40..=0x7e).contains(&b) => {
                            final_byte = b;
                            break;
                        }
                        Some(b) => params.push(b),
                        None => break,
                    }
                }
                match (final_byte, params.as_slice()) {
                    (b'A', _) => Key::Up,
                    (b'B', _) => Key::Down,
                    (b'C', _) => Key::Right,
                    (b'D', _) => Key::Left,
                    (b'H', _) | (b'~', b"1") | (b'~', b"7") => Key::Home,
                    (b'F', _) | (b'~', b"4") | (b'~', b"8") => Key::End,
                    (b'~', b"3") => Key::Delete,
                    (b'~', b"5") => Key::PageUp,
                    (b'~', b"6") => Key::PageDown,
                    _ => return Some(Ok(Key::Ctrl(0))),
                }
            }
            _ => Key::Esc,
        },
        b'\r' | b'\n' => Key::Enter,
        0x7f | 0x08 => Key::Backspace,
        b'\t' => Key::Ctrl(b'i'),
        b if b < 0x20 => Key::Ctrl(b + b'a' - 1),
        b if b < 0x80 => Key::Char(b as char),
        lead => {
            let len = match lead {
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf7 => 4,
                _ => return Some(Ok(Key::Char('\u{fffd}'))),
            };
            let mut buf = vec![lead];
            while buf.len() < len {
                match tty.read_event().await {
                    TtyEvent::Byte(b) => buf.push(b),
                    TtyEvent::Resize => continue,
                    TtyEvent::Closed => return None,
                }
            }
            Key::Char(
                std::str::from_utf8(&buf)
                    .ok()
                    .and_then(|s| s.chars().next())
                    .unwrap_or('\u{fffd}'),
            )
        }
    };
    Some(Ok(key))
}
