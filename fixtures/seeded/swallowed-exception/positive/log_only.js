// Defect: logged but never surfaced, so the caller sees undefined and cannot
// distinguish "no settings" from "settings failed to load".
export function loadSettings(path) {
  try {
    return JSON.parse(readFile(path));
  } catch (e) {
    console.error("failed to load settings", e);
  }
}
