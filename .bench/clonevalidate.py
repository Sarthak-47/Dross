"""Checks Dross's clone findings against an independent clone detector.

Companion to crossvalidate.py, which does the same for the two swallowed-
exception signals using ruff and oxlint. This covers the third signal that has
a tool with no stake in this repository asking a comparable question:

    near-duplicate-function  <->  jscpd  (token-based copy/paste detection)

WHAT THIS CAN AND CANNOT SHOW

jscpd compares token streams. Dross compares normalized ASTs, which means it
deliberately ignores identifier and literal differences — a renamed copy is the
case it exists to catch, and it is exactly the case jscpd is built not to see.
So the two tools overlap on one class of finding and diverge on another, and
the split is not symmetric:

  - jscpd agrees  ->  the two fragments are duplicated at the token level.
                      An independent detector calls it a clone. Strong
                      corroboration: this is a true positive by any definition
                      of the word "duplicate".

  - jscpd silent  ->  says nothing either way. The pair may be a renamed clone
                      (Dross right, jscpd blind by construction) or parallel
                      structure over different vocabulary (Dross wrong). This
                      script cannot tell those apart and does not try.

The number this prints is therefore a LOWER BOUND ON TRUE POSITIVES, not a
precision figure. A precision figure needs a labeller who can read the
remainder, and the one available is the same family of system Dross checks —
which is the conflict of interest this whole file exists to route around.

Reported plainly for that reason: "N of M corroborated" and "M - N untested",
never "N/M = X% precision".

SETTINGS

jscpd runs at --min-tokens 25 --min-lines 3, below its defaults (50/5). The
default misses short functions, and short functions are most of what a clone
detector is asked about here. Lowering a threshold makes the independent tool
MORE willing to agree, so it is stated rather than tuned quietly: a stricter
setting would report fewer corroborations, not more.

SETUP

    cd .bench && npm install        # jscpd, pinned in .bench/package.json

USAGE

    python .bench/clonevalidate.py .bench/findings-v10.jsonl
    python .bench/clonevalidate.py .bench/findings-v10.jsonl --shuffle

`--shuffle` keeps every finding's own span but repoints the counterpart at a
random file of the same language elsewhere in the same tree, at a random line.
Two functions picked that way have no reason to be duplicates, so the
corroboration rate must collapse; a harness that agrees with whatever it is
handed is measuring nothing. crossvalidate.py was caught this way once already,
scoring 94% off a mapping that was itself wrong.

Two weaker controls were tried first and are recorded here because they are the
ones a reader would reach for. Repointing at a different FINDING's counterpart
scored 30%, and keeping the file while moving the line scored 33% — neither
because the harness is loose, but because the corpus contains genuine clone
families (date-fns has a dozen mutually near-identical differenceInCalendar*
functions in files that are themselves almost entirely duplicated). A control
the subject matter can pass by accident measures the corpus, not the harness.
"""


import json
import pathlib
import random
import re
import subprocess
import sys
import tempfile

REPOS = pathlib.Path(__file__).parent / "repos"
# node runs the entry script directly rather than the .bin shim: the Windows
# shim is a .cmd, and dispatching one through subprocess produced a clean exit
# code with no work done and nothing on stderr.
JSCPD = pathlib.Path(__file__).parent / "node_modules" / "jscpd" / "bin" / "jscpd"

# "Normalized-AST similarity 100% against `trimSPorHTAB` at lib\helpers\x.js:5,"
COUNTERPART = re.compile(r"against `[^`]+` at (.+?):(\d+)")

MIN_TOKENS = "25"
MIN_LINES = "3"


def counterpart(finding: dict) -> tuple[str, int] | None:
    """The other half of the pair, parsed out of the evidence sentence."""
    m = COUNTERPART.search(finding.get("evidence", ""))
    if not m:
        return None
    # Findings are produced on Windows, where the engine renders paths with
    # backslashes. git needs forward slashes.
    return m.group(1).replace("\\", "/"), int(m.group(2))


def source_files_at(repo: str, commit: str, suffix: str) -> list[str]:
    """Every file of one extension in the tree, for the random-pair control."""
    out = subprocess.run(
        ["git", "ls-tree", "-r", "--name-only", commit],
        cwd=REPOS / repo,
        capture_output=True,
        timeout=60,
    )
    if out.returncode != 0:
        return []
    return [
        line
        for line in out.stdout.decode("utf-8", "replace").splitlines()
        if line.endswith(suffix)
    ]


def file_at(repo: str, commit: str, path: str) -> str | None:
    out = subprocess.run(
        ["git", "show", f"{commit}:{path}"],
        cwd=REPOS / repo,
        capture_output=True,
        timeout=60,
    )
    if out.returncode != 0:
        return None
    return out.stdout.decode("utf-8", "replace")


def jscpd_pairs(files: dict[str, str]) -> list[tuple[str, int, int, str, int, int]]:
    """Every clone jscpd finds across `files`, as (name, start, end) x 2.

    Both sides of the pair are written into one temp directory and jscpd is
    pointed at it, so a clone spanning two files is found the same way as one
    inside a single file.
    """
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        src = root / "src"
        src.mkdir()
        for name, text in files.items():
            target = src / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding="utf-8")

        out = root / "out"
        subprocess.run(
            [
                "node",
                str(JSCPD.resolve()),
                # POSIX separators, not str(). jscpd globs its input through
                # fast-glob, which reads a Windows backslash as an escape
                # character — so a native path matched no files at all and
                # jscpd exited 0 having scanned nothing. Every finding came
                # back uncorroborated, which would have read as a result.
                src.as_posix(),
                "--reporters", "json",
                "--output", out.as_posix(),
                "--min-tokens", MIN_TOKENS,
                "--min-lines", MIN_LINES,
                "--silent",
            ],
            capture_output=True,
            timeout=180,
        )
        report_path = out / "jscpd-report.json"
        if not report_path.exists():
            return []
        try:
            report = json.loads(report_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            return []

    pairs = []
    for d in report.get("duplicates", []):
        a, b = d.get("firstFile", {}), d.get("secondFile", {})
        if not a or not b:
            continue
        pairs.append(
            (
                pathlib.Path(a["name"]).name, a["start"], a["end"],
                pathlib.Path(b["name"]).name, b["start"], b["end"],
            )
        )
    return pairs


def overlaps(lo: int, hi: int, start: int, end: int) -> bool:
    return start <= hi and end >= lo


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    shuffle = "--shuffle" in sys.argv

    findings = [
        json.loads(line)
        for line in pathlib.Path(args[0]).read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    subjects = [f for f in findings if f["signal"] == "near-duplicate-function"]

    pairs = []
    no_counterpart = 0
    for f in subjects:
        other = counterpart(f)
        if other is None:
            no_counterpart += 1
            continue
        pairs.append((f, other))

    rng = random.Random(0)

    corroborated = 0
    untested = 0
    unreadable = 0
    examples: list[str] = []

    for f, (other_path, other_line) in pairs:
        if shuffle:
            # A random file of the same language elsewhere in the same tree,
            # at a random line. Two functions picked this way have no reason
            # to be duplicates of each other, so agreement here would mean the
            # comparison is not doing any work.
            candidates = source_files_at(
                f["repo"], f["commit"], pathlib.Path(f["file"]).suffix
            )
            candidates = [c for c in candidates if c not in (f["file"], other_path)]
            if not candidates:
                continue
            other_path = rng.choice(candidates)
            other_line = 1

        a_src = file_at(f["repo"], f["commit"], f["file"])
        b_src = (
            a_src
            if other_path == f["file"]
            else file_at(f["repo"], f["commit"], other_path)
        )
        if a_src is None or b_src is None:
            unreadable += 1
            continue

        if shuffle:
            other_line = rng.randrange(1, max(2, len(b_src.splitlines())))

        same_file = other_path == f["file"]
        a_name = "a" + pathlib.Path(f["file"]).suffix
        b_name = a_name if same_file else "b" + pathlib.Path(other_path).suffix
        files = {a_name: a_src} if same_file else {a_name: a_src, b_name: b_src}

        # Dross names the counterpart's first line but not its last. The pair
        # is a near-duplicate of the subject, so the subject's length is the
        # best available estimate of the counterpart's.
        span = f["end_line"] - f["start_line"]
        b_lo, b_hi = other_line, other_line + span

        hit = False
        for na, sa, ea, nb, sb, eb in jscpd_pairs(files):
            sides = [(na, sa, ea), (nb, sb, eb)]
            for (n1, s1, e1), (n2, s2, e2) in (sides, sides[::-1]):
                if (
                    n1 == a_name
                    and overlaps(f["start_line"], f["end_line"], s1, e1)
                    and n2 == b_name
                    and overlaps(b_lo, b_hi, s2, e2)
                ):
                    hit = True
                    break
            if hit:
                break

        if hit:
            corroborated += 1
            if len(examples) < 5:
                examples.append(
                    f"  {f['repo']}/{f['file']}:{f['start_line']} <-> {other_path}:{other_line}"
                )
        else:
            untested += 1

    total = corroborated + untested
    print(f"near-duplicate-function findings:      {len(subjects)}")
    print(f"  evidence carried no counterpart:     {no_counterpart}")
    print(f"  file unreadable at that commit:      {unreadable}")
    print(f"  compared against jscpd:              {total}")
    print()
    print(f"  corroborated by jscpd:               {corroborated}")
    print(f"  jscpd has nothing to say:            {untested}")
    if total:
        print()
        print(
            f"{corroborated} of {total} are duplicates at the token level too — a lower bound on\n"
            f"true positives, not a precision figure. The other {untested} are either renamed\n"
            "clones jscpd cannot see by construction, or false positives; this\n"
            "comparison does not distinguish them."
        )
    if examples:
        print("\ncorroborated, first few:")
        print("\n".join(examples))


if __name__ == "__main__":
    main()
