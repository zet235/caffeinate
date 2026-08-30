//! Small helpers shared by both binaries.

/// Convert a Rust string into the NUL-terminated UTF-16 buffer Win32 expects.
///
/// Interior NULs are dropped rather than passed through: Win32 reads the first
/// one as the end of the string, so keeping them would silently truncate
/// whatever followed and leave the caller none the wiser.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16()
        .filter(|&unit| unit != 0)
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_a_terminator() {
        assert_eq!(wide("hi"), vec![b'h' as u16, b'i' as u16, 0]);
    }

    #[test]
    fn an_empty_string_is_just_the_terminator() {
        assert_eq!(wide(""), vec![0]);
    }

    #[test]
    fn keeps_non_ascii_intact() {
        assert_eq!(wide("\u{5EFA}"), vec![0x5EFA, 0]);
    }

    #[test]
    fn drops_interior_nuls_rather_than_truncating() {
        // "a\0b" must not reach Win32 as "a".
        assert_eq!(wide("a\u{0}b"), vec![b'a' as u16, b'b' as u16, 0]);
    }
}
