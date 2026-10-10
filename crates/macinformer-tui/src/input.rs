//! Keyboard input parsing (VT100/xterm escape sequences + UTF-8).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PgUp,
    PgDn,
    Enter,
    Esc,
    Backspace,
    Delete,
    Tab,
    BackTab,
    F(u8),
}

/// Decode all complete key events from a raw byte buffer.
pub fn parse_keys(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0x1b => {
                let (key, consumed) = parse_escape(&bytes[i..]);
                keys.push(key);
                i += consumed;
            }
            b'\r' | b'\n' => {
                keys.push(Key::Enter);
                i += 1;
            }
            b'\t' => {
                keys.push(Key::Tab);
                i += 1;
            }
            0x7f | 0x08 => {
                keys.push(Key::Backspace);
                i += 1;
            }
            0x01..=0x1a => {
                keys.push(Key::Ctrl((b'a' + b - 1) as char));
                i += 1;
            }
            _ => {
                // UTF-8 sequence (1-byte ASCII included)
                let len = utf8_len(b);
                if len == 1 {
                    keys.push(Key::Char(b as char));
                    i += 1;
                } else if i + len <= bytes.len() {
                    if let Ok(s) = core::str::from_utf8(&bytes[i..i + len]) {
                        if let Some(c) = s.chars().next() {
                            keys.push(Key::Char(c));
                        }
                    }
                    i += len;
                } else {
                    i += 1;
                }
            }
        }
    }
    keys
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Parse one ESC-prefixed sequence; returns (key, bytes consumed).
fn parse_escape(b: &[u8]) -> (Key, usize) {
    if b.len() < 2 {
        return (Key::Esc, 1);
    }
    match b[1] {
        b'[' => {
            if b.len() < 3 {
                return (Key::Esc, 2);
            }
            match b[2] {
                b'A' => (Key::Up, 3),
                b'B' => (Key::Down, 3),
                b'C' => (Key::Right, 3),
                b'D' => (Key::Left, 3),
                b'H' => (Key::Home, 3),
                b'F' => (Key::End, 3),
                b'Z' => (Key::BackTab, 3),
                b'1'..=b'9' => {
                    // ESC [ <n> ~
                    let mut j = 2;
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j < b.len() && b[j] == b'~' {
                        let n: u32 = core::str::from_utf8(&b[2..j]).unwrap_or("0").parse().unwrap_or(0);
                        let key = match n {
                            1 | 7 => Key::Home,
                            2 => Key::Esc, // Insert — unused
                            3 => Key::Delete,
                            4 | 8 => Key::End,
                            5 => Key::PgUp,
                            6 => Key::PgDn,
                            11 => Key::F(1),
                            12 => Key::F(2),
                            13 => Key::F(3),
                            14 => Key::F(4),
                            15 => Key::F(5),
                            17 => Key::F(6),
                            18 => Key::F(7),
                            19 => Key::F(8),
                            20 => Key::F(9),
                            21 => Key::F(10),
                            _ => Key::Esc,
                        };
                        (key, j + 1)
                    } else {
                        (Key::Esc, j)
                    }
                }
                _ => (Key::Esc, 3),
            }
        }
        b'O' => {
            if b.len() < 3 {
                return (Key::Esc, 2);
            }
            let key = match b[2] {
                b'P' => Key::F(1),
                b'Q' => Key::F(2),
                b'R' => Key::F(3),
                b'S' => Key::F(4),
                b'H' => Key::Home,
                b'F' => Key::End,
                _ => Key::Esc,
            };
            (key, 3)
        }
        _ => (Key::Esc, 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_and_text() {
        let k = parse_keys(b"ab\x1b[A\x1b[B");
        assert_eq!(k, vec![Key::Char('a'), Key::Char('b'), Key::Up, Key::Down]);
    }

    #[test]
    fn ctrl_and_special() {
        let k = parse_keys(&[0x09, 0x0d, 0x11, 0x7f]);
        assert_eq!(k, vec![Key::Tab, Key::Enter, Key::Ctrl('q'), Key::Backspace]);
    }

    #[test]
    fn function_keys() {
        assert_eq!(parse_keys(b"\x1bOP"), vec![Key::F(1)]);
        assert_eq!(parse_keys(b"\x1b[15~"), vec![Key::F(5)]);
        assert_eq!(parse_keys(b"\x1b[5~"), vec![Key::PgUp]);
        assert_eq!(parse_keys(b"\x1b[Z"), vec![Key::BackTab]);
    }

    #[test]
    fn utf8() {
        let k = parse_keys("hé".as_bytes());
        assert_eq!(k, vec![Key::Char('h'), Key::Char('é')]);
    }
}
