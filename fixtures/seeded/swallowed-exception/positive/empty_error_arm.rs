// Defect: the Err arm discards the failure. Rust has no catch; the match arm
// that handles the error is where a Result is either dealt with or dropped.
pub fn load_port(raw: &str) -> u16 {
    match raw.parse::<u16>() {
        Ok(port) => port,
        Err(_) => {}
    }
    8080
}
