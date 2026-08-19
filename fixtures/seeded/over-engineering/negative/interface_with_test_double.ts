// Correct: a single production implementor plus a test double is a
// legitimate seam, not speculative generality.
interface Clock {
  now(): number;
}
class SystemClock implements Clock {
  now(): number { return Date.now(); }
}
class FakeClock implements Clock {
  now(): number { return 0; }
}
