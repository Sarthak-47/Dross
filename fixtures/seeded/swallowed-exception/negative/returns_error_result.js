// Correct: failure is returned explicitly, so the caller must handle it.
export function loadSettings(path) {
  try {
    return { ok: true, value: JSON.parse(readFile(path)) };
  } catch (e) {
    return { ok: false, error: e };
  }
}
