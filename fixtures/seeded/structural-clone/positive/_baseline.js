// Indexed as the pre-existing implementation the clone should match against.
export function computeTotal(items) {
  let total = 0;
  for (const item of items) {
    total += item.price * item.quantity;
  }
  return total;
}
