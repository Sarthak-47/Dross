/**
 * Rules in App.css whose absence is invisible in review and silent at runtime.
 *
 * A `<span>` is inline, and an inline box ignores width and height. Any bar or
 * swatch built from a span therefore needs an explicit display, or it renders
 * as nothing while the markup that sizes it looks perfectly correct.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const css = readFileSync(fileURLToPath(new URL("./App.css", import.meta.url)), "utf-8");

/**
 * The declarations of one rule, by exact selector, with comments stripped.
 *
 * These rules carry comments explaining what they replaced, and those quote the
 * old declarations — `.split__left` describes having been `flex: none`. Matching
 * the raw text finds the explanation instead of the rule.
 */
function rule(selector: string): string {
  const at = css.indexOf(`\n${selector} {`);
  expect(at, `${selector} is declared in App.css`).toBeGreaterThan(-1);
  return css.slice(at, css.indexOf("}", at)).replace(/\/\*[\s\S]*?\*\//g, "");
}

describe("elements sized by an inline style are not inline boxes", () => {
  /**
   * The bug: `.prec__fill` carried `style={{ width: "100%" }}` from the
   * component and computed to zero width for every signal, because a span
   * ignores it. The precision column is the Settings table's whole point, and
   * it was drawing an empty track beside every number.
   */
  it("gives the precision bar's fill a box to size", () => {
    expect(rule(".prec__fill")).toMatch(/display:\s*(block|flex|inline-block)/);
  });

  /** Same shape: the severity bar's segments are spans with inline widths. */
  it("gives the severity bar's segments a box to size", () => {
    for (const selector of [".prec__track", ".sevbar"]) {
      expect(rule(selector)).toMatch(/display:\s*(block|flex)|flex:/);
    }
  });
});

describe("the findings list stays scannable", () => {
  /**
   * A contract-change finding on a TypeScript union puts the whole type in the
   * message. One real finding rendered a 554px row and filled the list.
   */
  it("clamps an unselected row's message and evidence", () => {
    expect(css).toMatch(
      /\.finding:not\(\[aria-current="true"\]\)\s+\.finding__msg\s*\{[^}]*line-clamp/,
    );
    expect(css).toMatch(
      /\.finding:not\(\[aria-current="true"\]\)\s+\.finding__evidence\s*\{[^}]*line-clamp/,
    );
  });
});

describe("neither pane of the split can starve the other", () => {
  /**
   * `.split__left` was `flex: none` at 620px, so a 900px window left the
   * source pane 280px for a header needing 335px and pushed "Open in editor"
   * outside the window.
   */
  it("lets the findings list shrink, and caps its share", () => {
    const left = rule(".split__left");
    expect(left).not.toMatch(/flex:\s*none/);
    expect(left).toMatch(/max-width:\s*\d+%/);
    expect(left).toMatch(/min-width:/);
  });

  /** Source lines are `white-space: pre` and can be arbitrarily long. */
  it("scrolls long source lines inside the pane", () => {
    expect(rule(".source__scroll")).toMatch(/overflow-x:\s*auto/);
  });
});
