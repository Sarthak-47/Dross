// Defect: the failure path returns a value shaped like success.
export function parsePort(raw) {
  try {
    return Number.parseInt(raw, 10);
  } catch (e) {
    return 0;
  }
}
