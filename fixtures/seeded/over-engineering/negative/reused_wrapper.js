// Correct: a thin wrapper with several callers is genuine deduplication.
function fetchUser(id) {
  return getUser(id);
}
fetchUser(1);
fetchUser(2);
fetchUser(3);
