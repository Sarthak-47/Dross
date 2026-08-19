// Defect: the exception is discarded entirely — no log, rethrow, or fallback.
export function loadSettings(path) {
  try {
    return JSON.parse(readFile(path));
  } catch (e) {}
}
