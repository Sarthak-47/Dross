// Correct: an empty Err arm carrying an explanation is a decision the author
// wrote down — the same rule the other languages already follow.
pub fn cached_port(raw: &str) -> u16 {
    match raw.parse::<u16>() {
        Ok(port) => port,
        Err(_) => {
            // A malformed cache entry is the normal cold-start case.
        }
    }
    8080
}
