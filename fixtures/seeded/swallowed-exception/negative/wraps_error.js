// Correct: the original error is wrapped in a domain error and rethrown.
export function loadSettings(path) {
  try {
    return JSON.parse(readFile(path));
  } catch (e) {
    throw new ConfigError(`cannot read ${path}`, { cause: e });
  }
}
