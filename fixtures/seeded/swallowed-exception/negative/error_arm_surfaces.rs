// Correct: the Err arm hands the failure back to the caller, so nothing is
// concealed. Structurally identical to the positive case above.
pub fn load_port(raw: &str) -> Result<u16, std::num::ParseIntError> {
    match raw.parse::<u16>() {
        Ok(port) => Ok(port),
        Err(e) => Err(e),
    }
}
