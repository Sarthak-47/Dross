// Correct: compared against a fixture, not against the subject's own output.
it("matches the golden output", () => {
  expect(render(template)).toEqual(GOLDEN_OUTPUT);
});
