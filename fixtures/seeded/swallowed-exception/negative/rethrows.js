// Correct: logged for diagnostics, then rethrown so the caller still fails.
export function loadSettings(path) {
  try {
    return JSON.parse(readFile(path));
  } catch (e) {
    console.error("failed to load settings", e);
    throw e;
  }
}
