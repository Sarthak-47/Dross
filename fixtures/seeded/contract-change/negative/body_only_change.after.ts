// Correct: the contract is unchanged; only the implementation moved.
export function send(url: string): Response { return httpClient.get(url); }
