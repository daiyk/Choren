import { Fragment, type ReactNode } from "react";

/**
 * Renders the Markdown subset used by CHANGELOG.md and the release notes: headings, paragraphs,
 * bullet and numbered lists (nested by indentation), fenced code, inline code, bold, italic and links.
 * Everything becomes React elements, so release text can never inject HTML.
 */
export function Markdown({ source, className }: { source: string; className?: string }) {
  return <div className={className}>{blocks(source.replace(/<!--[\s\S]*?-->/g, "").split(/\r?\n/))}</div>;
}

const ITEM = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;
const BLOCK_START = /^\s*(#{1,6}\s|```|~~~)/;
const indent = (line: string) => line.length - line.trimStart().length;

function blocks(lines: string[]): ReactNode[] {
  const out: ReactNode[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }
    const fence = line.match(/^\s*(```|~~~)/);
    if (fence) {
      const code: string[] = [];
      for (i++; i < lines.length && !lines[i].trimStart().startsWith(fence[1]); i++) code.push(lines[i]);
      i++;
      out.push(<pre key={out.length}><code>{code.join("\n")}</code></pre>);
      continue;
    }
    const heading = line.match(/^\s*(#{1,6})\s+(.*?)\s*#*\s*$/);
    if (heading) {
      const Tag = heading[1].length <= 2 ? "h3" : "h4";
      out.push(<Tag key={out.length}>{inline(heading[2])}</Tag>);
      i++;
      continue;
    }
    const first = line.match(ITEM);
    if (first) {
      const ordered = /\d/.test(first[2]);
      const base = indent(line);
      const items: string[][] = [];
      let item: RegExpMatchArray | null;
      while (i < lines.length && (item = lines[i].match(ITEM)) && indent(lines[i]) === base && /\d/.test(item[2]) === ordered) {
        // Continuation lines lose the indentation up to where the item's text starts.
        const text = base + item[2].length + 1;
        const body = [item[3]];
        for (i++; i < lines.length; i++) {
          const next = lines[i];
          if (!next.trim()) {
            // A blank line continues the item only when indented content follows.
            if (indent(lines[i + 1] ?? "") > base && lines[i + 1].trim()) { body.push(""); continue; }
            break;
          }
          if (indent(next) <= base && (ITEM.test(next) || BLOCK_START.test(next))) break;
          body.push(next.slice(Math.min(indent(next), text)));
        }
        items.push(body);
      }
      const List = ordered ? "ol" : "ul";
      out.push(<List key={out.length}>{items.map((body, n) =>
        // Single-paragraph items render inline rather than as a nested <p>.
        <li key={n}>{body.some(l => !l.trim() || ITEM.test(l) || BLOCK_START.test(l))
          ? blocks(body) : inline(body.map(l => l.trim()).join(" "))}</li>)}</List>);
      continue;
    }
    const paragraph: string[] = [];
    while (i < lines.length && lines[i].trim() && !ITEM.test(lines[i]) && !BLOCK_START.test(lines[i]))
      paragraph.push(lines[i++].trim());
    out.push(<p key={out.length}>{inline(paragraph.join(" "))}</p>);
  }
  return out;
}

const INLINE = /\\([\\`*_[\]()#+\-.!])|`([^`]+)`|\*\*(.+?)\*\*|__(.+?)__|\*(?!\s)(.+?)\*|(?<![\w])_(?!\s)(.+?)_(?![\w])|\[([^\]]+)\]\(([^)\s]+)\)/g;

function inline(text: string): ReactNode {
  const parts: ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    if (m.index > last) parts.push(text.slice(last, m.index));
    const key = parts.length;
    if (m[1] !== undefined) parts.push(m[1]);
    else if (m[2] !== undefined) parts.push(<code key={key}>{m[2]}</code>);
    else if (m[3] !== undefined || m[4] !== undefined) parts.push(<strong key={key}>{inline(m[3] ?? m[4])}</strong>);
    else if (m[5] !== undefined || m[6] !== undefined) parts.push(<em key={key}>{inline(m[5] ?? m[6])}</em>);
    // The webview cannot open external pages, so links keep their text and show the address on hover.
    else parts.push(<span key={key} className="markdown-link" title={m[8]}>{inline(m[7])}</span>);
    last = m.index + m[0].length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return <Fragment>{parts}</Fragment>;
}
