# Defect: a new required parameter breaks every existing caller, and the
# breakage lives at the call sites rather than in this diff.
def send(url, retries):
    return get(url, retries)
