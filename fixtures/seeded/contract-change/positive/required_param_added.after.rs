// Defect: a new required parameter. Every existing call site fails to compile,
// which is loud in Rust but still a breaking change to the contract.
pub fn render(input: &str, width: usize) -> String {
    format!("{input}{width}")
}
