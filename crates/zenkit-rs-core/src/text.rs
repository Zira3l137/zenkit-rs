//! Text as the engine stores it: bytes in an unknown code page, and numbers written by C's
//! `printf` and read back by `strtol`/`strtof`.

/// Decodes bytes as Latin-1, which maps every byte to the char with the same value.
///
/// The engine stores names (files, textures, objects) as raw bytes in whatever code page the game
/// was localised with. Latin-1 is not always the right reading, but it is the one that can never
/// fail and can always be turned back into the original bytes, so lookups by name keep working.
pub fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// Encodes text as Latin-1, the reverse of [`latin1`]. Gives `None` if a character lies beyond
/// U+00FF and so has no Latin-1 byte.
pub fn to_latin1(text: &str) -> Option<Vec<u8>> {
    text.chars().map(|c| u8::try_from(c).ok()).collect()
}

/// Parses an integer like C's `strtol`: leading whitespace is skipped and parsing stops at the
/// first character that cannot continue the number, so `"12]"` is 12.
pub fn int(s: &[u8]) -> Option<i64> {
    let s = s.trim_ascii_start();
    let sign = usize::from(matches!(s.first(), Some(b'+' | b'-')));
    let digits = s[sign..].iter().take_while(|b| b.is_ascii_digit()).count();
    std::str::from_utf8(&s[..sign + digits]).ok()?.parse().ok()
}

/// Parses a float like C's `strtof`: leading whitespace is skipped and the longest prefix that
/// forms a number counts, so `"1.5e"` is 1.5. `inf` and `nan` count too, in any case.
pub fn float(s: &[u8]) -> Option<f32> {
    let s = s.trim_ascii_start();
    let (negative, unsigned) = match s.first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let special = |word: &[u8]| unsigned.get(..3).is_some_and(|w| w.eq_ignore_ascii_case(word));
    if special(b"inf") {
        return Some(if negative { f32::NEG_INFINITY } else { f32::INFINITY });
    }
    if special(b"nan") {
        return Some(if negative { -f32::NAN } else { f32::NAN });
    }
    let candidate = s
        .iter()
        .take_while(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
        .count();
    (1..=candidate).rev().find_map(|end| std::str::from_utf8(&s[..end]).ok()?.parse().ok())
}

/// Whitespace-separated floats; missing ones read as zero, like a failed C++ stream read.
pub fn floats<const N: usize>(s: &[u8]) -> [f32; N] {
    let mut parts = s.split(u8::is_ascii_whitespace).filter(|p| !p.is_empty());
    std::array::from_fn(|_| parts.next().and_then(float).unwrap_or(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_byte() {
        let bytes: Vec<u8> = (0..=255).collect();
        let text = latin1(&bytes);
        let back: Vec<u8> = text.chars().map(|c| c as u8).collect();
        assert_eq!(back, bytes);
        assert_eq!(to_latin1(&text), Some(bytes));
        assert_eq!(to_latin1("Gr\u{fc}\u{df}e"), Some(b"Gr\xFC\xDFe".to_vec()));
        assert_eq!(to_latin1("\u{416}"), None);
    }

    #[test]
    fn ints_stop_at_the_first_non_digit() {
        assert_eq!(int(b"  42"), Some(42));
        assert_eq!(int(b"-7 rest"), Some(-7));
        assert_eq!(int(b"12]"), Some(12));
        assert_eq!(int(b"x"), None);
        assert_eq!(int(b"-"), None);
    }

    #[test]
    fn floats_take_the_longest_valid_prefix() {
        assert_eq!(float(b"12.5"), Some(12.5));
        assert_eq!(float(b"9.9999997e-005"), Some(1e-4));
        assert_eq!(float(b"1.5e"), Some(1.5));
        assert_eq!(float(b"-3x"), Some(-3.0));
        assert_eq!(float(b"abc"), None);
        assert_eq!(float(b"-inf"), Some(f32::NEG_INFINITY));
        assert_eq!(float(b"Infinity"), Some(f32::INFINITY));
        assert!(float(b"NaN").unwrap().is_nan());
        assert!(float(b"-nan(ind)").unwrap().is_sign_negative());
    }

    #[test]
    fn missing_floats_are_zero() {
        assert_eq!(floats::<3>(b"1 2"), [1.0, 2.0, 0.0]);
        assert_eq!(floats::<2>(b" 512   256 extra"), [512.0, 256.0]);
    }
}
