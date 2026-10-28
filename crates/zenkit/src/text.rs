/// Decodes bytes as Latin-1, which maps every byte to the char with the same value.
///
/// The engine stores names (files, textures, objects) as raw bytes in whatever code page the game
/// was localised with. Latin-1 is not always the right reading, but it is the one that can never
/// fail and can always be turned back into the original bytes, so lookups by name keep working.
pub(crate) fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trips_every_byte() {
        let bytes: Vec<u8> = (0..=255).collect();
        let text = super::latin1(&bytes);
        let back: Vec<u8> = text.chars().map(|c| c as u8).collect();
        assert_eq!(back, bytes);
    }
}
