"""Apply verdicts to worksheet rows by id substring, with a justification."""
import json, pathlib, sys

ws = pathlib.Path(sys.argv[1])
labels_file = pathlib.Path(sys.argv[2])
labels = json.loads(labels_file.read_text(encoding="utf-8"))

rows = [json.loads(l) for l in ws.read_text(encoding="utf-8").splitlines() if l.strip()]
applied = 0
for r in rows:
    for key, (verdict, note) in labels.items():
        if r["id"] == key:
            r["verdict"] = verdict
            r["note"] = note
            applied += 1
            break
ws.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")
done = sum(1 for r in rows if r.get("verdict"))
print(f"applied {applied}; labeled {done}/{len(rows)}")
