// Correct: the expectation is a literal the implementation must produce.
it("slugifies a title", () => {
  expect(slugify("Hello World")).toBe("hello-world");
});
