// Correct: the parameter takes different values, so the generality is used.
function render(node, useCache) {
  return draw(node, useCache);
}
render(a, true);
render(b, false);
render(c, true);
