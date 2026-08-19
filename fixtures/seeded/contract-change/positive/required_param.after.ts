// Defect: a new required parameter breaks every existing call site, and the
// breakage is not visible anywhere in this diff.
export function send(url: string, retries: number): Response { return get(url); }
