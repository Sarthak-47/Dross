# Correct: Python duck typing means there is no declared interface to flag,
# and a base class with shared behaviour is not speculative generality.
class Repository:
    def __init__(self, conn):
        self.conn = conn

    def find(self, key):
        raise NotImplementedError

class SqlRepository(Repository):
    def find(self, key):
        return self.conn.query(key)

class CacheRepository(Repository):
    def find(self, key):
        return self.conn.get(key)
