// Defect: same logic as computeTotal with every identifier renamed — the
// classic agent reinvention that textual search will not catch.
export function sumBasket(entries) {
  let acc = 0;
  for (const entry of entries) {
    acc += entry.price * entry.quantity;
  }
  return acc;
}
