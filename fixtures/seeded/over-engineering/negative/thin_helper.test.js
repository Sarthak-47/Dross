// Correct: a fixture helper used by a single test is not needless generality.
function buildUser(overrides) {
  return makeUser(overrides);
}

it("creates a user", () => {
  expect(buildUser({ id: 1 }).id).toBe(1);
});
