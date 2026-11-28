/** JSON syntax-highlighted viewer for the Task JSON tab. */

import type { JsonToken } from "./jsonTokens";
import { tokenizeJson } from "./jsonTokens";

export function JsonViewer({ value }: { value: unknown }) {
  const json = JSON.stringify(value, null, 2) ?? "null";
  const tokens = tokenizeJson(json);
  return (
    <pre className="json-viewer" aria-label="Task JSON" tabIndex={0}>
      {tokens.map((token, index) => renderToken(token, index))}
    </pre>
  );
}

function renderToken(token: JsonToken, index: number) {
  if (token.kind === "plain") {
    return <span key={index}>{token.text}</span>;
  }
  return (
    <span key={index} className={`json-token json-${token.kind}`}>
      {token.text}
    </span>
  );
}
