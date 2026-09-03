"""Checks Dross's findings against independent linters, not against a labeller.

The precision figures in docs/BENCHMARK_RESULTS.md were produced by a single
labeller reading findings and deciding which were real — and that labeller was
the same family of system Dross is built to check. That is a conflict of
interest the document states plainly, and no amount of further labelling by the
same party removes it.

This removes the judgement instead. Two of Dross's signals have close
equivalents in linters maintained by large communities and used on millions of
lines:

    empty-catch-body        <->  ruff S110            (try-except-pass)   Python
    overly-broad-catch-type <->  ruff BLE001 or E722  (blind / bare)      Python
    empty-catch-body        <->  oxlint no-empty      (ESLint's rule)     JS/TS

Two rules for breadth, because ruff splits the question: BLE001 covers
`except Exception`, E722 covers a bare `except:`. Mapping only the first
counted httpx's bare handlers as disagreements when both tools in fact agreed —
a fault in this file, not in Dross, and the reason a comparison needs checking
as carefully as the thing it compares.

Where Dross and ruff both fire on the same handler, the finding is corroborated
by a tool with no stake in this repository. That is not a precision measurement
— agreement is not truth, and the rules are not identical — but it is evidence
that does not come from me.

The alternative was considered and rejected. The standard automated substitute
is the "closed-warning heuristic": call a warning real if it disappears from a
later revision. Kang, Aw and Lo (ICSE 2022, arXiv:2202.05982) manually checked
1,357 such warnings and found only 49% agreed with human annotators, with 38%
removed incidentally by unrelated edits. It would have been a worse oracle than
the one it replaced.

    python .bench/crossvalidate.py .bench/findings-final2.jsonl
"""

import collections
import json
import pathlib
import subprocess
import sys
import tempfile

REPOS = pathlib.Path(__file__).parent / "repos"
RUFF = pathlib.Path(__file__).parent.parent / ".venv" / "Scripts" / "python.exe"

# Dross signal -> the ruff rules that ask the same question. Corroborated if
# any of them fires on the handler.
PYTHON_EQUIVALENT = {
    "empty-catch-body": ("S110",),
    "overly-broad-catch-type": ("BLE001", "E722"),
}

# JavaScript has no typed catch, so only the empty-body question transfers.
JS_EQUIVALENT = {"empty-catch-body": ("no-empty",)}

JS_SUFFIXES = (".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx")
OXLINT = pathlib.Path(__file__).parent.parent / "apps" / "desktop" / "node_modules" / ".bin"


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


def ruff_lines(source: str, rules: tuple[str, ...]) -> set[int]:
    """Lines where ruff raises any of `rules`, for one file's contents."""
    with tempfile.TemporaryDirectory() as tmp:
        target = pathlib.Path(tmp) / "subject.py"
        target.write_text(source, encoding="utf-8")
        out = subprocess.run(
            [
                str(RUFF), "-m", "ruff", "check",
                # Isolated so the analysed repository's own ruff configuration
                # cannot switch the rule off and turn a disagreement into a
                # silent pass.
                "--isolated",
                "--select", ",".join(rules),
                "--output-format", "json",
                str(target),
            ],
            capture_output=True,
            timeout=120,
        )
        try:
            return {r["location"]["row"] for r in json.loads(out.stdout or "[]")}
        except json.JSONDecodeError:
            return set()


def oxlint_lines(source: str, suffix: str) -> set[int]:
    """Lines where oxlint's no-empty fires, for one file's contents."""
    with tempfile.TemporaryDirectory() as tmp:
        target = pathlib.Path(tmp) / f"subject{suffix}"
        target.write_text(source, encoding="utf-8")
        out = subprocess.run(
            [
                str(OXLINT / ("oxlint.cmd" if sys.platform == "win32" else "oxlint")),
                # Only the rule under comparison, and none of the repository's
                # own configuration.
                "-A", "all", "-D", "no-empty", "-f", "json", str(target),
            ],
            capture_output=True,
            timeout=120,
        )
        try:
            report = json.loads(out.stdout or "{}")
        except json.JSONDecodeError:
            return set()
        lines = set()
        for d in report.get("diagnostics", []):
            if "no-empty" not in d.get("code", ""):
                continue
            for label in d.get("labels", []):
                lines.add(label["span"]["line"])
        return lines


def main() -> None:
    findings = [
        json.loads(line)
        for line in pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]

    subjects = [
        f
        for f in findings
        if (f["signal"] in PYTHON_EQUIVALENT and f["file"].endswith(".py"))
        or (f["signal"] in JS_EQUIVALENT and f["file"].endswith(JS_SUFFIXES))
    ]
    print(f"comparable findings: {len(subjects)}")

    cache: dict[tuple[str, str, str], set[int]] = {}
    agree: collections.Counter[str] = collections.Counter()
    total: collections.Counter[str] = collections.Counter()
    unreadable = 0
    misses = []

    for f in subjects:
        python = f["file"].endswith(".py")
        rules = PYTHON_EQUIVALENT[f["signal"]] if python else JS_EQUIVALENT[f["signal"]]
        tool = "ruff" if python else "oxlint"
        key = (f["repo"], f["commit"], f["file"] + "/".join(rules))
        if key not in cache:
            source = file_at(f["repo"], f["commit"], f["file"])
            if source is None:
                unreadable += 1
                continue
            suffix = pathlib.Path(f["file"]).suffix
            cache[key] = (
                ruff_lines(source, rules) if python else oxlint_lines(source, suffix)
            )
        lines = cache[key]

        label = f"{f['signal']} ({tool} {' or '.join(rules)})"
        total[label] += 1
        # Dross reports the handler's own span; ruff reports the `except` line.
        # Accept a hit anywhere in the handler.
        if any(f["start_line"] <= line <= f["end_line"] for line in lines):
            agree[label] += 1
        else:
            misses.append(f)

    print(f"files unreadable at that commit: {unreadable}\n")
    print(f"{'Dross signal (independent rule)':<48}{'agreed':>8}{'of':>6}{'rate':>8}")
    for label in sorted(total):
        n = total[label]
        print(f"{label:<48}{agree[label]:>8}{n:>6}{100 * agree[label] // n:>7}%")
    grand = sum(total.values())
    if grand:
        print(f"{'all':<48}{sum(agree.values()):>8}{grand:>6}{100 * sum(agree.values()) // grand:>7}%")

    if misses:
        print(f"\nnot corroborated ({len(misses)}), first few:")
        for f in misses[:8]:
            print(f"  {f['repo']}/{f['file']}:{f['start_line']}  {f['signal']}")


if __name__ == "__main__":
    main()
