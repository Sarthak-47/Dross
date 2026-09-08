"""Checks Dross's cyclomatic complexity against an independent implementation.

Third in the family with crossvalidate.py and clonevalidate.py, and the only
one that checks a *number* rather than a finding.

`Metrics::cyclomatic` calls itself "McCabe cyclomatic complexity: 1 + number of
branch points". That is not a judgement call or a heuristic — it is a named
metric from a 1976 paper with a settled definition, so an independent
implementation either agrees or one of them is wrong. ruff ships `C901`, which
is the `mccabe` algorithm, and reports the figure for every function when
`max-complexity` is set to 0.

This matters beyond tidiness. `complexity-to-problem-size-outlier` scores a
change against a z-distribution built from this number. A metric that is
systematically inflated relative to the standard does not necessarily break the
z-score — a constant offset cancels — but one that is inflated *unevenly*, by
how many `else` branches a function happens to have, moves functions around in
the distribution for reasons unrelated to complexity.

WHAT AGREEMENT MEANS HERE

Unlike the finding comparisons, this one admits a right answer. mccabe is the
reference implementation of a published metric; where the two disagree, Dross
is the one that has to justify itself.

SETUP

    python -m venv .venv && .venv/Scripts/pip install -r .bench/requirements.txt

USAGE

    python .bench/complexityvalidate.py flask requests click
    python .bench/complexityvalidate.py --all
"""

import collections
import json
import pathlib
import re
import sqlite3
import subprocess
import sys

REPOS = pathlib.Path(__file__).parent / "repos"
RUFF = pathlib.Path(__file__).parent.parent / ".venv" / "Scripts" / "python.exe"
DROSS = pathlib.Path(__file__).parent.parent / "target" / "release" / "dross.exe"

# "`github_link` is too complex (4 > 0)"
COMPLEXITY = re.compile(r"`([^`]+)` is too complex \((\d+) > \d+\)")


def dross_metrics(repo: pathlib.Path) -> dict[tuple[str, str, int], int]:
    """Every indexed Python function's cyclomatic figure, from the index."""
    index = repo / ".dross" / "index.sqlite"
    if not index.exists():
        subprocess.run([str(DROSS.resolve()), "index"], cwd=repo, capture_output=True)
    if not index.exists():
        return {}
    con = sqlite3.connect(index)
    out = {}
    for path, name, line, cyclo in con.execute(
        "SELECT path, name, start_line, cyclomatic FROM functions"
    ):
        if not path.endswith(".py"):
            continue
        # The engine writes native separators; ruff is invoked with POSIX ones.
        out[(path.replace("\\", "/"), name, line)] = cyclo
    con.close()
    return out


def mccabe_metrics(repo: pathlib.Path) -> dict[tuple[str, str, int], int]:
    """The same figure from ruff's C901, which is the mccabe algorithm."""
    out = subprocess.run(
        [
            str(RUFF), "-m", "ruff", "check",
            # Isolated so the analysed repository's own ruff settings cannot
            # change the threshold out from under the comparison.
            "--isolated",
            "--select", "C901",
            "--config", "lint.mccabe.max-complexity=0",
            "--output-format", "json",
            ".",
        ],
        cwd=repo,
        capture_output=True,
        timeout=600,
    )
    try:
        rows = json.loads(out.stdout or "[]")
    except json.JSONDecodeError:
        return {}

    found = {}
    for row in rows:
        m = COMPLEXITY.search(row.get("message", ""))
        if not m:
            continue
        path = (row.get("filename") or "").replace("\\", "/")
        # ruff gives absolute paths; the index gives repo-relative ones.
        rel = path.split(repo.name + "/", 1)[-1] if repo.name + "/" in path else path
        found[(rel, m.group(1), row["location"]["row"])] = int(m.group(2))
    return found


def main() -> None:
    names = [a for a in sys.argv[1:] if not a.startswith("--")]
    if "--all" in sys.argv:
        names = sorted(p.name for p in REPOS.iterdir() if p.is_dir())

    agree = 0
    differ: collections.Counter[int] = collections.Counter()
    examples: list[str] = []
    compared = 0

    for name in names:
        repo = REPOS / name
        if not repo.is_dir():
            print(f"  no such clone: {name}")
            continue
        ours = dross_metrics(repo)
        theirs = mccabe_metrics(repo)
        shared = set(ours) & set(theirs)
        if not shared:
            continue
        print(f"{name:<14} dross {len(ours):>5}  mccabe {len(theirs):>5}  matched {len(shared):>5}")
        for key in shared:
            compared += 1
            delta = ours[key] - theirs[key]
            if delta == 0:
                agree += 1
            else:
                differ[delta] += 1
                if len(examples) < 8:
                    path, fn, line = key
                    examples.append(
                        f"  {name}/{path}:{line} {fn}  dross={ours[key]} mccabe={theirs[key]}"
                    )

    if not compared:
        print("nothing compared")
        return

    print()
    print(f"functions compared:            {compared}")
    print(f"identical:                     {agree}  ({100 * agree // compared}%)")
    print(f"different:                     {compared - agree}")
    if differ:
        print("\ndross minus mccabe, commonest first:")
        for delta, n in differ.most_common(8):
            print(f"  {delta:+d}  {n:>6}")
    if examples:
        print("\nexamples:")
        print("\n".join(examples))


if __name__ == "__main__":
    main()
