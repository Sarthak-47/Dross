"""Builds a self-contained labelling checklist from a worksheet.

Turns `dross-bench label` output into one HTML page a human can open with no
repo, no tooling: every flagged item carries its code and Dross's reasoning
inline, three choices (real / false alarm / unsure), and an export button that
emits verdicts to paste back for `dross-bench report`.

Only the four disabled signals are included — they are the ones whose on/off
decision is unsettled. The verdict must be the reader's own: an AI grading
Dross proves nothing, because Dross exists to check AI output.

    python .bench/build_checklist.py .bench/worksheet.jsonl checklist.html
"""

import html
import json
import pathlib
import subprocess
import sys

REPOS = pathlib.Path(__file__).parent / "repos"

DISABLED = [
    "near-duplicate-function",
    "silent-optimistic-return",
    "single-implementation-abstraction",
    "complexity-to-problem-size-outlier",
]

# One plain sentence per signal: what "real" and "false alarm" mean, so the
# reader is applying a consistent rule and not guessing at intent.
GUIDANCE = {
    "near-duplicate-function": (
        "Two functions that look structurally alike.",
        "REAL if they do the same thing and a change to one ought to change the other too.",
        "FALSE ALARM if they are deliberately parallel — sibling APIs, adapters, "
        "per-locale or per-platform variants — that happen to share a shape.",
    ),
    "silent-optimistic-return": (
        "On failure the function returns a normal-looking value instead of signalling the error.",
        "REAL if that value makes a caller believe the operation succeeded when it did not.",
        "FALSE ALARM if the value is the documented contract — a cache miss, a retry "
        "signal, empty-on-bad-input, a get-or-none lookup.",
    ),
    "single-implementation-abstraction": (
        "An abstract type with only one implementation in this repository.",
        "REAL in ordinary app code: one impl, one caller, no sign anything else will ever "
        "extend it — the abstraction only adds indirection.",
        "FALSE ALARM if it is a deliberate extension point — a library base class, a plugin "
        "or dependency-injection boundary, a testing seam — meant to be subclassed elsewhere.",
    ),
    "complexity-to-problem-size-outlier": (
        "A change far more complex than this repo's norm for its size.",
        "REAL if the complexity is accidental and out of proportion to what the change does.",
        "FALSE ALARM if the complexity is inherent — a parser, a state machine, a "
        "compatibility shim, a concurrency or security constraint.",
    ),
}

TIER = {
    "near-duplicate-function": "Concrete hazard",
    "silent-optimistic-return": "Concrete hazard",
    "single-implementation-abstraction": "Review prompt — judgement call",
    "complexity-to-problem-size-outlier": "Review prompt — judgement call",
}


def file_at(repo: str, commit: str, path: str) -> list[str] | None:
    out = subprocess.run(
        ["git", "show", f"{commit}:{path}"],
        cwd=REPOS / repo,
        capture_output=True,
        timeout=60,
    )
    if out.returncode != 0:
        return None
    return out.stdout.decode("utf-8", "replace").splitlines()


def window(lines: list[str], start: int, end: int, before: int = 14, after: int = 2) -> str:
    a = max(1, start - before)
    b = min(len(lines), end + after)
    return "\n".join(f"{n:5} {lines[n - 1]}" for n in range(a, b + 1))


def counterpart(evidence: str):
    import re
    m = re.search(r"against `([^`]+)` at (.+?):(\d+)", evidence)
    if not m:
        return None
    return m.group(1), m.group(2).replace("\\", "/"), int(m.group(3))


def code_for(f: dict) -> str:
    lines = file_at(f["repo"], f["commit"], f["file"])
    if lines is None:
        return "(file not present at this commit)"
    primary = window(lines, f["start_line"], f["end_line"])
    if f["signal"] != "near-duplicate-function":
        return primary
    cp = counterpart(f.get("evidence", ""))
    if not cp:
        return primary
    name, cpath, cline = cp
    other_lines = lines if cpath == f["file"] else file_at(f["repo"], f["commit"], cpath)
    span = f["end_line"] - f["start_line"]
    other = (
        window(other_lines, cline, cline + span)
        if other_lines
        else "(counterpart not present at this commit)"
    )
    return (
        f"— THIS function ({f['file']}:{f['start_line']}) —\n{primary}\n\n"
        f"— the one it allegedly duplicates ({cpath}:{cline}) —\n{other}"
    )


def main() -> None:
    worksheet = pathlib.Path(sys.argv[1])
    out_path = pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else pathlib.Path("checklist.html")
    rows = [json.loads(l) for l in worksheet.read_text(encoding="utf-8").splitlines() if l.strip()]
    rows = [r["finding"] if "finding" in r else r for r in rows]

    items = []
    for sig in DISABLED:
        for f in [r for r in rows if r["signal"] == sig]:
            items.append(f)

    counts = {sig: sum(1 for f in items if f["signal"] == sig) for sig in DISABLED}

    # Section per signal, so the reader carries one rule at a time rather than
    # re-deriving it every card.
    sections = []
    idx = 0
    for sig in DISABLED:
        group = [f for f in items if f["signal"] == sig]
        if not group:
            continue
        what, real, false = GUIDANCE[sig]
        hazard = TIER[sig].startswith("Concrete")
        chip = "hazard" if hazard else "prompt"
        cards = []
        for f in group:
            fid = f.get("id") or f"{f['repo']}:{f['file']}:{f['start_line']}"
            code = html.escape(code_for(f))
            cards.append(f"""
    <article class="card" data-id="{html.escape(fid)}" data-signal="{html.escape(sig)}">
      <header class="card-top">
        <span class="loc">{html.escape(f['repo'])}<span class="sep">/</span>{html.escape(f['file'])}:{f['start_line']}</span>
      </header>
      <p class="msg">{html.escape(f['message'])}</p>
      <p class="why"><span class="lbl">why flagged</span>{html.escape(f.get('evidence',''))}</p>
      <details><summary>code</summary><pre><code>{code}</code></pre></details>
      <div class="verdict" role="radiogroup" aria-label="verdict">
        <label class="opt opt-tp"><input type="radio" name="v-{idx}" value="tp"><span>Real problem</span></label>
        <label class="opt opt-fp"><input type="radio" name="v-{idx}" value="fp"><span>False alarm</span></label>
        <label class="opt opt-un"><input type="radio" name="v-{idx}" value="unsure"><span>Unsure</span></label>
        <input class="note" type="text" placeholder="note (optional)" aria-label="note">
      </div>
    </article>""")
            idx += 1
        sections.append(f"""
  <section class="sig-section">
    <div class="sig-head">
      <div class="sig-name">
        <span class="eyebrow">{'signal — off' }</span>
        <h2>{html.escape(sig)}</h2>
      </div>
      <span class="chip chip-{chip}">{html.escape(TIER[sig])}</span>
      <span class="sig-count">{len(group)}</span>
    </div>
    <p class="what">{html.escape(what)}</p>
    <div class="rulebox">
      <p><span class="tag tag-tp">Real</span>{html.escape(real)}</p>
      <p><span class="tag tag-fp">False alarm</span>{html.escape(false)}</p>
    </div>
    {''.join(cards)}
  </section>""")

    total = len(items)
    doc = f"""<title>Dross Signal Review</title>
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@400;500;600&family=IBM+Plex+Mono:wght@400;500&display=swap">
<style>
  /* Grounded in Dross's own desktop tokens — ember accent, warn/info/ok
     signals, IBM Plex. Dark-first, all three theme states handled. */
  :root, :root[data-theme="dark"] {{
    --bg:#121417; --panel:#171a1e; --panel2:#1c1f24; --raise:#22262c;
    --line:#282d34; --text:#e7e9ea; --dim:#98a0a8; --faint:#828a91;
    --ember:#e0692a; --ember-wash:#2a1a12; --ember-dim:#7d3a19;
    --warn:#c79433; --warn-wash:#26200f; --info:#5c8ba3; --info-wash:#14202a;
    --ok:#6c9a6b; --ok-wash:#16220f; --on-accent:#191c20;
  }}
  @media (prefers-color-scheme: light) {{
    :root:not([data-theme="dark"]) {{
      --bg:#f4f2ee; --panel:#ffffff; --panel2:#efece7; --raise:#e6e2db;
      --line:#d6d1c8; --text:#191c20; --dim:#5b6268; --faint:#666c72;
      --ember:#a84716; --ember-wash:#f6e9df; --ember-dim:#d9a684;
      --warn:#8a6512; --warn-wash:#f5eddb; --info:#38657d; --info-wash:#e6eef3;
      --ok:#4c7a4b; --ok-wash:#e7efe4; --on-accent:#ffffff;
    }}
  }}
  :root[data-theme="light"] {{
    --bg:#f4f2ee; --panel:#ffffff; --panel2:#efece7; --raise:#e6e2db;
    --line:#d6d1c8; --text:#191c20; --dim:#5b6268; --faint:#666c72;
    --ember:#a84716; --ember-wash:#f6e9df; --ember-dim:#d9a684;
    --warn:#8a6512; --warn-wash:#f5eddb; --info:#38657d; --info-wash:#e6eef3;
    --ok:#4c7a4b; --ok-wash:#e7efe4; --on-accent:#ffffff;
  }}
  :root {{
    --sans:"IBM Plex Sans", system-ui, -apple-system, sans-serif;
    --mono:"IBM Plex Mono", ui-monospace, "SF Mono", Consolas, monospace;
  }}
  * {{ box-sizing: border-box; }}
  body {{ background: var(--bg); color: var(--text); font-family: var(--sans);
         line-height: 1.55; margin: 0; }}
  .wrap {{ max-width: 860px; margin: 0 auto; padding: 0 20px 80px; }}
  a {{ color: var(--ember); }}
  h1 {{ font-size: 27px; font-weight: 600; letter-spacing: -0.01em; margin: 0 0 4px;
        text-wrap: balance; }}
  h2 {{ font-family: var(--mono); font-size: 17px; font-weight: 500; margin: 0; }}
  .eyebrow {{ font-size: 11px; letter-spacing: 0.14em; text-transform: uppercase;
             color: var(--faint); }}

  /* Masthead */
  .mast {{ padding: 40px 0 20px; border-bottom: 1px solid var(--line); }}
  .kicker {{ font-family: var(--mono); font-size: 12px; letter-spacing: 0.1em;
             text-transform: uppercase; color: var(--ember); margin: 0 0 10px; }}
  .lede {{ color: var(--dim); max-width: 62ch; margin: 14px 0 0; }}
  .lede strong {{ color: var(--text); font-weight: 600; }}
  .note-box {{ margin: 20px 0 0; padding: 14px 18px; background: var(--ember-wash);
               border: 1px solid var(--ember-dim); border-radius: 8px;
               color: var(--text); max-width: 62ch; }}
  .note-box .eyebrow {{ color: var(--ember); display: block; margin-bottom: 4px; }}

  /* Sticky progress bar */
  #bar {{ position: sticky; top: 0; z-index: 20; margin-top: 20px;
          background: color-mix(in srgb, var(--bg) 88%, transparent);
          backdrop-filter: blur(8px); border-bottom: 1px solid var(--line);
          display: flex; gap: 14px; align-items: center; flex-wrap: wrap;
          padding: 12px 0; }}
  #progress {{ font-family: var(--mono); font-variant-numeric: tabular-nums;
               font-size: 13px; color: var(--dim); }}
  #meter {{ flex: 1; min-width: 120px; height: 4px; border-radius: 2px;
            background: var(--raise); overflow: hidden; }}
  #meter i {{ display: block; height: 100%; width: 0; background: var(--ember);
              transition: width 0.2s; }}
  button {{ font-family: var(--sans); font-size: 13px; font-weight: 500;
            padding: 7px 14px; border-radius: 6px; cursor: pointer;
            border: 1px solid var(--line); background: var(--raise); color: var(--text); }}
  button.primary {{ background: var(--ember); border-color: var(--ember-dim);
                    color: var(--on-accent); }}
  button:focus-visible, .opt input:focus-visible + span,
  summary:focus-visible {{ outline: 2px solid var(--ember); outline-offset: 2px; }}

  /* Signal section */
  .sig-section {{ margin: 40px 0 0; }}
  .sig-head {{ display: flex; align-items: baseline; gap: 12px; flex-wrap: wrap;
               padding-bottom: 10px; border-bottom: 1px solid var(--line); }}
  .sig-count {{ margin-left: auto; font-family: var(--mono); font-size: 13px;
                color: var(--faint); }}
  .chip {{ font-size: 11px; letter-spacing: 0.04em; padding: 3px 9px; border-radius: 100px;
           font-weight: 500; }}
  .chip-hazard {{ background: var(--warn-wash); color: var(--warn);
                  border: 1px solid color-mix(in srgb, var(--warn) 40%, transparent); }}
  .chip-prompt {{ background: var(--info-wash); color: var(--info);
                  border: 1px solid color-mix(in srgb, var(--info) 40%, transparent); }}
  .what {{ color: var(--dim); margin: 12px 0 0; }}
  .rulebox {{ margin: 12px 0 4px; padding: 12px 16px; background: var(--panel2);
              border-radius: 8px; display: grid; gap: 6px; }}
  .rulebox p {{ margin: 0; font-size: 14px; color: var(--dim); }}
  .tag {{ display: inline-block; font-size: 11px; font-weight: 600; letter-spacing: 0.03em;
          padding: 1px 7px; border-radius: 4px; margin-right: 8px; }}
  .tag-tp {{ background: var(--ok-wash); color: var(--ok); }}
  .tag-fp {{ background: var(--info-wash); color: var(--info); }}

  /* Cards */
  .card {{ background: var(--panel); border: 1px solid var(--line); border-radius: 10px;
           padding: 16px 18px; margin: 14px 0; }}
  .card.done {{ border-color: var(--ember-dim); }}
  .card-top {{ display: flex; }}
  .loc {{ font-family: var(--mono); font-size: 12px; color: var(--faint); }}
  .loc .sep {{ color: var(--line); padding: 0 1px; }}
  .msg {{ font-weight: 600; margin: 8px 0 6px; font-size: 15px; }}
  .why {{ color: var(--dim); margin: 0; font-size: 14px; }}
  .why .lbl {{ font-size: 10px; letter-spacing: 0.1em; text-transform: uppercase;
               color: var(--faint); display: inline-block; margin-right: 8px; }}
  details {{ margin: 12px 0; }}
  summary {{ cursor: pointer; font-family: var(--mono); font-size: 12px; color: var(--info);
             width: max-content; }}
  pre {{ margin: 10px 0 0; padding: 14px; background: var(--panel2);
         border: 1px solid var(--line-soft, var(--line)); border-radius: 8px;
         overflow-x: auto; }}
  pre code {{ font-family: var(--mono); font-size: 12px; line-height: 1.5;
              color: var(--text); white-space: pre; }}

  /* Verdict control — a segmented choice, clearly interactive */
  .verdict {{ display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin-top: 14px; }}
  .opt {{ position: relative; }}
  .opt input {{ position: absolute; opacity: 0; inset: 0; cursor: pointer; }}
  .opt span {{ display: block; padding: 6px 14px; border-radius: 6px; font-size: 13px;
               border: 1px solid var(--line); background: var(--raise); color: var(--dim);
               cursor: pointer; user-select: none; }}
  .opt-tp input:checked + span {{ background: var(--ok); color: var(--on-accent);
               border-color: var(--ok); font-weight: 600; }}
  .opt-fp input:checked + span {{ background: var(--info); color: var(--on-accent);
               border-color: var(--info); font-weight: 600; }}
  .opt-un input:checked + span {{ background: var(--raise); color: var(--text);
               border-color: var(--faint); font-weight: 600; }}
  .note {{ flex: 1; min-width: 150px; padding: 6px 10px; font-family: var(--sans);
           font-size: 13px; background: var(--panel2); color: var(--text);
           border: 1px solid var(--line); border-radius: 6px; }}
  #out {{ width: 100%; height: 200px; margin-top: 16px; padding: 12px;
          font-family: var(--mono); font-size: 12px; background: var(--panel2);
          color: var(--text); border: 1px solid var(--line); border-radius: 8px; }}
  @media (prefers-reduced-motion: reduce) {{ #meter i {{ transition: none; }} }}
</style>

<div class="wrap">
  <header class="mast">
    <p class="kicker">Dross · independent signal review</p>
    <h1>Which of these are real problems?</h1>
    <p class="lede">Four of Dross's checks ship <strong>switched off</strong>. Their accuracy
    was only ever graded by the tool's own author — and Dross exists to check what AI writes,
    so an author (or an AI) grading it proves nothing. <strong>Your independent read is what
    decides whether any can be trusted enough to turn on.</strong></p>
    <div class="note-box">
      <span class="eyebrow">what to do</span>
      For each item, read the code and Dross's reason, then choose <strong>Real problem</strong>
      or <strong>False alarm</strong> — or <strong>Unsure</strong>. About {total} items, a few
      seconds each. Your choices save in this browser as you go; when you're done, press
      <strong>Export verdicts</strong> and send back the box that appears.
    </div>
  </header>

  <div id="bar">
    <span id="progress">0 of {total}</span>
    <span id="meter"><i></i></span>
    <button class="primary" onclick="doExport()">Export verdicts</button>
    <button onclick="if(confirm('Clear every answer on this page?')){{localStorage.clear();location.reload();}}">Reset</button>
  </div>
{''.join(sections)}
  <textarea id="out" hidden placeholder="Your verdicts will appear here after you press Export verdicts."></textarea>
</div>

<script>
  const cards = [...document.querySelectorAll('.card')];
  const KEY = id => 'dross-review:' + id;
  cards.forEach(c => {{
    let saved = null;
    try {{ saved = JSON.parse(localStorage.getItem(KEY(c.dataset.id)) || 'null'); }} catch (e) {{}}
    if (saved) {{
      const r = c.querySelector('input[value="' + saved.verdict + '"]');
      if (r) r.checked = true;
      if (saved.note) c.querySelector('.note').value = saved.note;
    }}
    c.classList.toggle('done', !!c.querySelector('input:checked'));
    c.addEventListener('input', () => save(c));
  }});
  function save(c) {{
    const v = c.querySelector('input[type=radio]:checked');
    try {{
      localStorage.setItem(KEY(c.dataset.id), JSON.stringify({{
        verdict: v ? v.value : null, note: c.querySelector('.note').value }}));
    }} catch (e) {{}}
    c.classList.toggle('done', !!v);
    progress();
  }}
  function progress() {{
    const done = cards.filter(c => c.querySelector('input[type=radio]:checked')).length;
    document.getElementById('progress').textContent = done + ' of ' + cards.length;
    document.querySelector('#meter i').style.width = (100 * done / cards.length) + '%';
  }}
  function doExport() {{
    const lines = cards.map(c => {{
      const v = c.querySelector('input[type=radio]:checked');
      return JSON.stringify({{ id: c.dataset.id, signal: c.dataset.signal,
        verdict: v ? v.value : null, note: c.querySelector('.note').value || null }});
    }});
    const t = document.getElementById('out');
    t.hidden = false; t.value = lines.join('\\n'); t.focus(); t.select();
  }}
  progress();
</script>"""
    out_path.write_text(doc, encoding="utf-8")
    print(f"{len(items)} items -> {out_path}")
    for sig in DISABLED:
        print(f"  {sig}: {counts[sig]}")


if __name__ == "__main__":
    main()
