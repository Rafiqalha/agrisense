import React from "react";

function renderInline(text: string, keyPrefix: string): React.ReactNode[] {
  const pattern = /(`[^`\n]+`|\*\*[^*\n]+\*\*|__[^_\n]+__|\*[^*\n]+\*|_[^_\n]+_|\[[^\]\n]+\]\([^)\n]+\))/g;
  const parts = text.split(pattern);
  return parts.map((part, i) => {
    const key = `${keyPrefix}-${i}`;
    if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
      return <code key={key}>{part.slice(1, -1)}</code>;
    }
    if ((part.startsWith("**") && part.endsWith("**")) || (part.startsWith("__") && part.endsWith("__"))) {
      if (part.length > 4) return <strong key={key}>{part.slice(2, -2)}</strong>;
    }
    if ((part.startsWith("*") && part.endsWith("*")) || (part.startsWith("_") && part.endsWith("_"))) {
      if (part.length > 2) return <em key={key}>{part.slice(1, -1)}</em>;
    }
    const link = /^\[([^\]\n]+)\]\(([^)\n]+)\)$/.exec(part);
    if (link) {
      const href = link[2].trim();
      if (/^(https?:|mailto:)/.test(href)) {
        return (
          <a key={key} href={href} target="_blank" rel="noreferrer">
            {link[1]}
          </a>
        );
      }
      return <React.Fragment key={key}>{part}</React.Fragment>;
    }
    return <React.Fragment key={key}>{part}</React.Fragment>;
  });
}

function isTableSep(line: string): boolean {
  const t = line.trim();
  return /^\|?[\s:|-]+\|?$/.test(t) && t.includes("-");
}

function splitRow(line: string): string[] {
  let t = line.trim();
  if (t.startsWith("|")) t = t.slice(1);
  if (t.endsWith("|")) t = t.slice(0, -1);
  return t.split("|").map((c) => c.trim());
}

function parseAlign(sep: string): ("left" | "center" | "right")[] {
  return splitRow(sep).map((c) => {
    if (c.startsWith(":") && c.endsWith(":") && c.length > 2) return "center";
    if (c.endsWith(":")) return "right";
    return "left";
  });
}

export function Markdown({ text }: { text: string }): React.ReactElement {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const blocks: React.ReactNode[] = [];
  let i = 0;
  let k = 0;
  const key = () => `b${k++}`;

  while (i < lines.length) {
    const line = lines[i];

    if (/^\s*```/.test(line)) {
      const buf: string[] = [];
      i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) {
        buf.push(lines[i]);
        i++;
      }
      i++;
      blocks.push(
        <pre key={key()}>
          <code>{buf.join("\n")}</code>
        </pre>
      );
      continue;
    }

    if (/^\s*\|.*\|\s*$/.test(line) && i + 1 < lines.length && isTableSep(lines[i + 1])) {
      const head = splitRow(line);
      const align = parseAlign(lines[i + 1]);
      i += 2;
      const rows: string[][] = [];
      while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) {
        rows.push(splitRow(lines[i]));
        i++;
      }
      blocks.push(
        <div key={key()} className="md-table-wrap">
          <table>
          <thead>
            <tr>
              {head.map((c, ci) => (
                <th key={ci} style={{ textAlign: align[ci] ?? "left" }}>
                  {renderInline(c, `${key()}-h${ci}`)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((r, ri) => (
              <tr key={ri}>
                {r.map((c, ci) => (
                  <td key={ci} style={{ textAlign: align[ci] ?? "left" }}>
                    {renderInline(c, `${key()}-c${ri}-${ci}`)}
                  </td>
                ))}
              </tr>
            ))}
            </tbody>
          </table>
        </div>
      );
      continue;
    }

    const h = /^(#{1,4})\s+(.*)$/.exec(line.trim());
    if (h) {
      const level = h[1].length;
      const kids = renderInline(h[2], key());
      if (level === 1) blocks.push(<h1 key={key()}>{kids}</h1>);
      else if (level === 2) blocks.push(<h2 key={key()}>{kids}</h2>);
      else if (level === 3) blocks.push(<h3 key={key()}>{kids}</h3>);
      else blocks.push(<h4 key={key()}>{kids}</h4>);
      i++;
      continue;
    }

    if (/^\s*(---|\*\*\*|___)\s*$/.test(line)) {
      blocks.push(<hr key={key()} />);
      i++;
      continue;
    }

    if (/^\s*>/.test(line)) {
      const buf: string[] = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) {
        buf.push(lines[i].replace(/^\s*>\s?/, ""));
        i++;
      }
      blocks.push(<blockquote key={key()}>{renderInline(buf.join("\n"), key())}</blockquote>);
      continue;
    }

    if (/^\s*([-*+•]\s+|\d+[.)]\s+)/.test(line)) {
      const items: { ordered: boolean; text: string }[] = [];
      while (i < lines.length && /^\s*([-*+•]\s+|\d+[.)]\s+)/.test(lines[i])) {
        const m = /^\s*([-*+•]\s+|\d+[.)]\s+)/.exec(lines[i]);
        items.push({
          ordered: /^\d/.test(m?.[1]?.trim() ?? ""),
          text: lines[i].replace(/^\s*([-*+•]\s+|\d+[.)]\s+)/, ""),
        });
        i++;
      }
      const ordered = items[0]?.ordered ?? false;
      const lis = items.map((it, li) => <li key={li}>{renderInline(it.text, `${key()}-li${li}`)}</li>);
      blocks.push(ordered ? <ol key={key()}>{lis}</ol> : <ul key={key()}>{lis}</ul>);
      continue;
    }

    if (line.trim() === "") {
      i++;
      continue;
    }

    const buf: string[] = [line];
    i++;
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      !/^(#{1,4}\s+|```|>|\s*([-*+•]\s+|\d+[.)]\s+)|\s*(---|\*\*\*|___)\s*$)/.test(lines[i]) &&
      !(/^\s*\|.*\|\s*$/.test(lines[i]) && i + 1 < lines.length && isTableSep(lines[i + 1]))
    ) {
      buf.push(lines[i]);
      i++;
    }
    blocks.push(<p key={key()}>{renderInline(buf.join("\n"), key())}</p>);
  }

  return <div className="md">{blocks}</div>;
}
