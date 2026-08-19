# Defect: catches the broad base type where only ValueError is expected.
def parse_port(raw):
    try:
        return int(raw)
    except Exception as exc:
        logging.error("bad port: %s", exc)
