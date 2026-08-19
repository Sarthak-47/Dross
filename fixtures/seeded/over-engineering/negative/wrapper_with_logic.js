// Correct: the wrapper validates before delegating, so it is not a pure hop.
function fetchUser(id) {
  assertValidId(id);
  return getUser(id);
}
fetchUser(1);
