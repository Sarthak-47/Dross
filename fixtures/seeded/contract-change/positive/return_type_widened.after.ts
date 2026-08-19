// Defect: callers written against User will not handle null.
export function findUser(id: string): User | null { return db.find(id); }
