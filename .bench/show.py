"""Render worksheet rows with the real source at each finding's commit.

Labeling from a finding's message alone would be guessing. This pulls the
actual code out of the repository at the exact commit so each label is made
against what a reviewer would have seen.
"""
import json, subprocess, sys, pathlib
sys.stdout.reconfigure(encoding="utf-8", errors="replace")

REPOS = pathlib.Path(__file__).parent / "repos"

def blob(repo, commit, path):
    try:
        out = subprocess.run(
            ["git", "show", f"{commit}:{path}"],
            cwd=REPOS / repo, capture_output=True, timeout=30,
        )
        if out.returncode != 0:
            return None
        return out.stdout.decode("utf-8", "replace").splitlines()
    except Exception:
        return None

def main():
    ws = pathlib.Path(sys.argv[1])
    want = sys.argv[2] if len(sys.argv) > 2 else None
    ctx = int(sys.argv[3]) if len(sys.argv) > 3 else 8

    for i, line in enumerate(ws.read_text(encoding="utf-8").splitlines()):
        if not line.strip():
            continue
        r = json.loads(line)
        if want and r["signal"] != want:
            continue
        lines = blob(r["repo"], r["commit"], r["file"].replace("\\", "/"))
        print("=" * 78)
        print(f"#{i} id={r['id']}")
        print(f"{r['repo']} {r['file']}:{r['start_line']}-{r['end_line']}  [{r['signal']}]")
        print(f"msg: {r['message']}")
        if r.get("related"):
            print(f"related: {r['related']}")
        if lines is None:
            print("  <source unavailable at this commit>")
            continue
        lo = max(1, r["start_line"] - ctx)
        hi = min(len(lines), r["end_line"] + ctx)
        for n in range(lo, hi + 1):
            mark = ">>" if r["start_line"] <= n <= r["end_line"] else "  "
            print(f"{mark}{n:5}| {lines[n-1]}")

main()
