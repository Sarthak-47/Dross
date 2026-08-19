// Defect: non-awaiting callers now receive a Promise instead of a User, and
// in JS this fails at use, not at the call site.
export async function loadUser(id: string): User { return db.get(id); }
