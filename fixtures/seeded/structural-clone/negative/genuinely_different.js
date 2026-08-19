// Correct: superficially similar shape, materially different logic — it
// filters, applies a discount, and short-circuits.
export function discountedTotal(items, rate) {
  let total = 0;
  for (const item of items) {
    if (!item.eligible) {
      continue;
    }
    total += item.price * item.quantity * (1 - rate);
    if (total > MAX_ORDER) {
      throw new OrderTooLargeError(total);
    }
  }
  return total;
}
