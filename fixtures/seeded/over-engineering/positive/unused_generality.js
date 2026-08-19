// Defect: the flag exists but every call site passes the same value.
function render(node, useCache) {
  return draw(node, useCache);
}
render(a, true);
render(b, true);
render(c, true);
