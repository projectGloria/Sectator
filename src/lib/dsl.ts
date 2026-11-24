// Display-side tokenizer for the search bar's operator pills. The authoritative
// parse happens in Rust (parser/query.rs); this only splits text for chips.

export const OPERATORS = ["file", "cat", "res", "codec", "src", "seeds", "size", "site", "source"] as const;
export type Operator = (typeof OPERATORS)[number];

export interface Token {
  raw: string;
  op?: Operator;
  value?: string;
  excluded?: boolean;
}

/** Split on whitespace, keeping `"quoted phrases"` (including `op:"x y"`) intact. */
export function tokenize(text: string): string[] {
  const out: string[] = [];
  let cur = "";
  let quoted = false;
  for (const ch of text) {
    if (ch === '"') quoted = !quoted;
    if (!quoted && /\s/.test(ch)) {
      if (cur) out.push(cur);
      cur = "";
    } else {
      cur += ch;
    }
  }
  if (cur) out.push(cur);
  return out;
}

export function classify(raw: string): Token {
  const m = /^([a-z]+):(.+)$/i.exec(raw);
  if (m && (OPERATORS as readonly string[]).includes(m[1].toLowerCase())) {
    return { raw, op: m[1].toLowerCase() as Operator, value: m[2].replace(/^"|"$/g, "") };
  }
  if (raw.length > 1 && raw.startsWith("-")) return { raw, excluded: true, value: raw.slice(1).replace(/^"|"$/g, "") };
  return { raw };
}

export function operatorTokens(text: string): Token[] {
  return tokenize(text).map(classify).filter((t) => t.op || t.excluded);
}

export function removeToken(text: string, raw: string): string {
  const toks = tokenize(text);
  const i = toks.indexOf(raw);
  if (i >= 0) toks.splice(i, 1);
  return toks.join(" ");
}

/** Replace any existing `op:` token with `op:value` (or drop it when value is empty). */
export function setOperator(text: string, op: Operator, value: string | null): string {
  const toks = tokenize(text).filter((t) => classify(t).op !== op);
  if (value) toks.push(`${op}:${/\s/.test(value) ? `"${value}"` : value}`);
  return toks.join(" ");
}

export function operatorValue(text: string, op: Operator): string | null {
  return tokenize(text).map(classify).find((t) => t.op === op)?.value ?? null;
}
