## What this changes

<!-- And why. A change to a check should say what it now reports or stops
     reporting, not only how it is implemented. -->

## Evidence

<!-- For a change to a check, the thing that matters is whether it moved
     precision or only volume. Suppressing findings is easy; suppressing false
     ones is the work.

     - Which signals changed, and by how many findings
     - Whether the seeded corpus still passes (`cargo test --workspace`)
     - If you removed findings, whether you read any of them against their
       source to confirm they were false -->

## Checklist

- [ ] `cargo test --workspace` and `npm test --prefix apps/desktop` pass
- [ ] `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings` are clean
- [ ] New tests fail when the code they cover is broken — a test that cannot
      fail reports coverage that does not exist
- [ ] Numbers stated in docs or UI copy are ones that were measured
