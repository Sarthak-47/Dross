# Correct: a narrow exception with a documented, intentional fallback that is
# itself the meaningful result, not a disguised failure.
def parse_port(raw, default):
    try:
        return int(raw)
    except ValueError as exc:
        raise ConfigError(f"invalid port {raw!r}") from exc
