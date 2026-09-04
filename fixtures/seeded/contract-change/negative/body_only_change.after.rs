// Correct: the body changed and the contract did not, so nothing is reported.
pub fn render(input: &str) -> String {
    let trimmed = input.trim();
    trimmed.to_owned()
}
