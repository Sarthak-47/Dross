# Defect: a bare except also swallows KeyboardInterrupt and SystemExit.
def load_settings(path):
    try:
        return parse(read(path))
    except:
        pass
