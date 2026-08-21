/**
 * Checks the design tokens themselves, because two of the three theme bugs
 * found in this file were invisible from the markup: the light palette was
 * unreachable (nothing ever set `data-theme`, and there was no media query),
 * and `--faint` failed contrast against every surface it was used on.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const css = readFileSync(fileURLToPath(new URL("./theme.css", import.meta.url)), "utf-8");

/** Pulls the `--name: value;` pairs out of one brace-delimited block. */
function tokens(block: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [, name, value] of block.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
    out[name] = value.trim();
  }
  return out;
}

/** The text between a selector's braces, matched by depth rather than by
 * guessing at indentation. */
function block(selector: string): string {
  const start = css.indexOf(selector);
  if (start === -1) throw new Error(`selector ${selector} not found in theme.css`);
  const open = css.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < css.length; i += 1) {
    if (css[i] === "{") depth += 1;
    else if (css[i] === "}") {
      depth -= 1;
      if (depth === 0) return css.slice(open + 1, i);
    }
  }
  throw new Error(`unbalanced braces after ${selector}`);
}

const dark = tokens(block(':root[data-theme="dark"]'));
const mediaLight = tokens(block(':root:not([data-theme="dark"])'));
const attrLight = tokens(block(':root[data-theme="light"]'));

// --- contrast -----------------------------------------------------------

function relativeLuminance(hex: string): number {
  const channels = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const linear = channels.map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [relativeLuminance(a), relativeLuminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Surfaces that text is actually painted on. */
const SURFACES = ["--panel", "--panel2", "--bg"] as const;
/** Tokens used for text. */
const INK = ["--text", "--dim", "--faint"] as const;

describe("the light theme is reachable", () => {
  /**
   * The bug: the light palette existed only behind `[data-theme="light"]`, and
   * nothing in the app ever sets that attribute, so no user could ever see it.
   */
  it("responds to the operating system's preference", () => {
    expect(css).toMatch(/@media \(prefers-color-scheme: light\)/);
  });

  it("still lets an explicit choice override the system in both directions", () => {
    expect(css).toContain(':root:not([data-theme="dark"])');
    expect(css).toContain(':root[data-theme="light"]');
  });

  it("defines the same tokens in both light blocks", () => {
    expect(Object.keys(mediaLight).sort()).toEqual(Object.keys(attrLight).sort());
  });

  /** Plain CSS cannot share the two declaration lists, so this is the guard. */
  it("gives every token the same value in both light blocks", () => {
    expect(mediaLight).toEqual(attrLight);
  });

  it("covers every token the dark theme defines", () => {
    expect(Object.keys(attrLight).sort()).toEqual(Object.keys(dark).sort());
  });
});

describe("text meets WCAG AA against the surfaces it sits on", () => {
  for (const [themeName, palette] of [
    ["dark", dark],
    ["light", attrLight],
  ] as const) {
    for (const ink of INK) {
      for (const surface of SURFACES) {
        it(`${themeName}: ${ink} on ${surface}`, () => {
          const ratio = contrast(palette[ink], palette[surface]);
          expect(
            ratio,
            `${palette[ink]} on ${palette[surface]} is ${ratio.toFixed(2)}:1`,
          ).toBeGreaterThanOrEqual(4.5);
        });
      }
    }
  }
});
