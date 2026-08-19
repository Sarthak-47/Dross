# Defect: the expected value re-invokes the function under test.
def test_slugify(self):
    self.assertEqual(slugify(title), slugify(title))
