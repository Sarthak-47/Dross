// Defect: the expected value is computed by calling the function under test.
it("normalizes whitespace", () => {
  expect(normalize(input)).toEqual(normalize(input).trim());
});
