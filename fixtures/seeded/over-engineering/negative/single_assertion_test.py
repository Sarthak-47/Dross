# Correct: a test that performs one assertion is shaped like a pass-through
# wrapper but is not indirection — the shape is normal in test code.
def test_roundtrip(self):
    self.assertEqual(decode(encode(payload)), payload)
