// Correct: the new parameter is optional, so existing call sites still work.
export function send(url: string, retries?: number): Response { return get(url); }
