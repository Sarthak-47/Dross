# Correct: the expected side calls a different, independently-verified helper.
def test_roundtrip(self):
    self.assertEqual(decode(encode(payload)), payload)
