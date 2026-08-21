"""List findings a previous run reported that the current run no longer does.

A drop in finding count is not by itself an improvement: suppressing true
positives lowers the count too. This pulls out exactly what disappeared, so
each one can be read against the real source and confirmed to have been a
false positive.

    python .bench/suppressed.py .bench/findings.jsonl .bench/findings-r6.jsonl [signal]
"""

import collections
import json
import pathlib
import sys


def load(path):
    return [
        json.loads(line)
        for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def main():
    before, after = load(sys.argv[1]), load(sys.argv[2])
    signal = sys.argv[3] if len(sys.argv) > 3 else None

    # Restricted to repositories both runs covered: a run that stopped early
    # would otherwise look like a huge suppression.
    common = {r["repo"] for r in before} & {r["repo"] for r in after}
    seen = {r["id"] for r in after if r["repo"] in common}

    gone = [r for r in before if r["repo"] in common and r["id"] not in seen]
    if signal:
        gone = [r for r in gone if r["signal"] == signal]

    by_signal = collections.Counter(r["signal"] for r in gone)
    print(f"repositories compared: {len(common)}")
    print(f"no longer reported:    {len(gone)}")
    for sig, n in by_signal.most_common():
        print(f"  {sig:<40}{n:>6}")
    print()

    out = pathlib.Path(".bench/suppressed.jsonl")
    out.write_text(
        "".join(json.dumps(r) + "\n" for r in gone),
        encoding="utf-8",
    )
    print(f"wrote {out} — read them with show.py")


if __name__ == "__main__":
    main()
