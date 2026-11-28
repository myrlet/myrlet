/**
 * Pure-TS JSON tokenizer behind the Task JSON tab.
 *
 * It never parses or rewrites the value: it only classifies spans of the
 * already-serialized text so the viewer can color them. Anything the pattern
 * does not recognize stays `plain` and is rendered verbatim.
 */

export type JsonTokenKind = "plain" | "key" | "string" | "number" | "literal" | "punct";

export interface JsonToken {
  readonly text: string;
  readonly kind: JsonTokenKind;
}

const JSON_TOKEN_PATTERN =
  /("(?:\\.|[^"\\])*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b|([{}[\],:])/g;

export function tokenizeJson(json: string): JsonToken[] {
  const tokens: JsonToken[] = [];
  let cursor = 0;
  JSON_TOKEN_PATTERN.lastIndex = 0;
  let match = JSON_TOKEN_PATTERN.exec(json);

  while (match !== null) {
    const [whole, stringToken, colonToken, numberToken, literalToken, punctToken] =
      match as unknown as [
        string,
        string | undefined,
        string | undefined,
        string | undefined,
        string | undefined,
        string | undefined,
      ];

    if (match.index > cursor) {
      tokens.push({ text: json.slice(cursor, match.index), kind: "plain" });
    }
    if (stringToken !== undefined) {
      tokens.push({ text: stringToken, kind: colonToken !== undefined ? "key" : "string" });
      if (colonToken !== undefined) {
        tokens.push({ text: colonToken, kind: "punct" });
      }
    } else if (numberToken !== undefined) {
      tokens.push({ text: numberToken, kind: "number" });
    } else if (literalToken !== undefined) {
      tokens.push({ text: literalToken, kind: "literal" });
    } else if (punctToken !== undefined) {
      tokens.push({ text: punctToken, kind: "punct" });
    }
    cursor = match.index + whole.length;
    match = JSON_TOKEN_PATTERN.exec(json);
  }

  if (cursor < json.length) {
    tokens.push({ text: json.slice(cursor), kind: "plain" });
  }
  return tokens;
}
