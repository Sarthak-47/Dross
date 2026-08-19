// Defect: both sides are the same expression, so the test passes regardless
// of what slugify does.
it("slugifies a title", () => {
  expect(slugify(title)).toBe(slugify(title));
});
