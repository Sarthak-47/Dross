# Correct: the new parameter has a default, so existing callers still work.
def send(url, retries=3):
    return get(url, retries)
