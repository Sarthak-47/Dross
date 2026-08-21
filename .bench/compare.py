"""Compare two benchmark runs by signal, restricted to repos both covered."""
import json, collections, sys, pathlib

def load(p):
    return [json.loads(l) for l in pathlib.Path(p).read_text(encoding="utf-8").splitlines() if l.strip()]

a, b = load(sys.argv[1]), load(sys.argv[2])
ra, rb = {r["repo"] for r in a}, {r["repo"] for r in b}
common = ra & rb
a = [r for r in a if r["repo"] in common]
b = [r for r in b if r["repo"] in common]

ca = collections.Counter(r["signal"] for r in a)
cb = collections.Counter(r["signal"] for r in b)

print(f"repositories compared: {len(common)}")
print(f"before: {len(a):>6}   after: {len(b):>6}   "
      f"reduction: {100*(len(a)-len(b))//max(len(a),1)}%")
print()
print(f"{'signal':<40}{'before':>8}{'after':>8}{'change':>9}")
for sig in sorted(set(ca) | set(cb), key=lambda s: -ca.get(s, 0)):
    x, y = ca.get(sig, 0), cb.get(sig, 0)
    pct = "—" if x == 0 else f"{100*(y-x)//x:+d}%"
    print(f"{sig:<40}{x:>8}{y:>8}{pct:>9}")
