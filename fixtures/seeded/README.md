# Seeded-defect corpus

Ground truth for recall measurement. Precision can be measured by labeling
findings the tool emits; recall cannot, because that sample contains no false
negatives by construction. This corpus supplies the missing half.

Layout:

- `<check>/positive/` — code containing exactly one instance of the defect the
  check targets. Every file here **must** produce a finding. A file that
  produces none is a false negative.
- `<check>/negative/` — code that resembles the defect but is correct. Nothing
  here may produce a finding for that check. A finding here is a false
  positive.

The negative cases matter more than the positive ones. Any check can reach
100% recall by flagging everything; the negatives are what make the number
mean something.

Each file starts with a comment stating what it tests and why.
