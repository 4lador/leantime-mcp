/**
 * Deterministic Markdown → HTML converter.
 *
 * Produces exactly the HTML subset understood by Leantime's TipTap editor
 * (h1–h4, p, ul/ol/li, task lists, blockquote, pre/code, hr, br, and the
 * inline marks strong/em/s/code/a). Raw HTML in the source is always
 * escaped — output formatting is fully determined by this module.
 */

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

const PLACEHOLDER = "\x00";

function isValidLinkUrl(url: string): boolean {
  return /^(https?:\/\/|mailto:)/i.test(url);
}

/** Inline rendering: escape, then apply code spans, links, and marks. */
function renderInline(text: string): string {
  const codeSpans: string[] = [];

  let out = escapeHtml(text);

  // Code spans are extracted first so their content is protected from
  // all further inline rules. Supports 1-3 backtick delimiters.
  out = out.replace(/(`{1,3})([^`\n]+?)\1/g, (_m, _delim: string, code: string) => {
    codeSpans.push(`<code>${code}</code>`);
    return `${PLACEHOLDER}${codeSpans.length - 1}${PLACEHOLDER}`;
  });

  // Links. "![…](…)" is left untouched (images are not supported).
  out = out.replace(
    /(?<!!)\[([^\]\n]+)\]\(([^)\s]+)\)/g,
    (match, label: string, url: string) => {
      if (!isValidLinkUrl(url)) return match;
      return `<a href="${url}">${label}</a>`;
    },
  );

  // Bold (boundary-anchored so snake_case and __dunder__ stay literal).
  out = out.replace(
    /(^|[\s(])(\*\*|__)(?=\S)([\s\S]*?\S)\2(?=$|[\s).,!?;:])/gm,
    (_m, pre: string, _mark: string, inner: string) => `${pre}<strong>${inner}</strong>`,
  );

  // Italic, same anchoring rules.
  out = out.replace(
    /(^|[\s(])(\*|_)(?=\S)([^*\n]*?\S)\2(?=$|[\s).,!?;:])/gm,
    (_m, pre: string, _mark: string, inner: string) => `${pre}<em>${inner}</em>`,
  );

  // Strikethrough.
  out = out.replace(/~~(?=\S)([\s\S]*?\S)~~/g, "<s>$1</s>");

  // Line breaks inside a block.
  out = out.replace(/\n/g, "<br />");

  // Restore code spans.
  out = out.replace(
    new RegExp(`${PLACEHOLDER}(\\d+)${PLACEHOLDER}`, "g"),
    (_m, idx: string) => codeSpans[Number(idx)],
  );

  return out;
}

interface ListItem {
  content: string;
  childLines: string[];
  task?: boolean;
  checked?: boolean;
}

const ITEM_RE = /^(\s*)([-*]|\d+\.)\s+(.*)$/;

function isListItemLine(line: string): boolean {
  return ITEM_RE.test(line);
}

function indentOf(line: string): number {
  return line.length - line.trimStart().length;
}

function parseListItems(
  lines: string[],
  start: number,
  indent: number,
): { items: ListItem[]; next: number } {
  const items: ListItem[] = [];
  let i = start;

  while (i < lines.length) {
    const match = lines[i].match(ITEM_RE);
    if (!match) break;
    if (match[1].length !== indent) break;

    let content = match[3];
    let task: boolean | undefined;
    let checked: boolean | undefined;
    if (match[2] === "-" || match[2] === "*") {
      const taskMatch = content.match(/^\[([ xX])\]\s+(.*)$/);
      if (taskMatch) {
        task = true;
        checked = taskMatch[1].toLowerCase() === "x";
        content = taskMatch[2];
      }
    }
    i++;

    // Lazy continuation: plain non-blank lines extend the item text.
    while (
      i < lines.length && !isBlankLine(lines[i]) && !isListItemLine(lines[i]) &&
      indentOf(lines[i]) <= indent
    ) {
      content += ` ${lines[i].trim()}`;
      i++;
    }

    // Deeper-indented lines become the item's nested block.
    const childLines: string[] = [];
    while (i < lines.length) {
      const line = lines[i];
      if (isBlankLine(line)) {
        const next = lines[i + 1] ?? "";
        if (indentOf(next) > indent) {
          childLines.push("");
          i++;
          continue;
        }
        break;
      }
      if (indentOf(line) > indent) {
        childLines.push(line.slice(Math.min(indent + 2, line.length - line.trimStart().length)));
        i++;
        continue;
      }
      break;
    }
    while (childLines.length > 0 && childLines[childLines.length - 1] === "") {
      childLines.pop();
    }

    items.push({ content, childLines, task, checked });
  }

  return { items, next: i };
}

function renderList(items: ListItem[], ordered: boolean): string {
  const isTask = !ordered && items.some((item) => item.task);
  const open = isTask ? '<ul data-type="taskList">' : ordered ? "<ol>" : "<ul>";

  const body = items.map((item) => {
    const inner = renderInline(item.content);
    const nested = item.childLines.length > 0 ? parseLines(item.childLines) : "";
    const tag = isTask
      ? `<li data-type="taskItem" data-checked="${item.checked === true}">`
      : "<li>";
    return `${tag}<p>${inner}</p>${nested}</li>`;
  }).join("");

  return `${open}${body}${isTask ? "</ul>" : ordered ? "</ol>" : "</ul>"}`;
}

function isBlankLine(line: string): boolean {
  return line.trim() === "";
}

/** Block-level parser over a list of lines. */
function parseLines(lines: string[]): string {
  const out: string[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    if (isBlankLine(line)) {
      i++;
      continue;
    }

    // Fenced code block.
    const fence = line.match(/^```(\w*)\s*$/);
    if (fence) {
      const lang = fence[1];
      const code: string[] = [];
      i++;
      while (i < lines.length && !/^```\s*$/.test(lines[i])) {
        code.push(lines[i]);
        i++;
      }
      i++; // closing fence (or EOF)
      const cls = lang ? ` class="language-${lang}"` : "";
      out.push(`<pre><code${cls}>${escapeHtml(code.join("\n"))}</code></pre>`);
      continue;
    }

    // ATX heading (# to ####, deeper levels clamp to h4).
    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      const level = Math.min(heading[1].length, 4);
      out.push(`<h${level}>${renderInline(heading[2].trim())}</h${level}>`);
      i++;
      continue;
    }

    // Horizontal rule.
    if (/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      out.push("<hr>");
      i++;
      continue;
    }

    // Blockquote (a blank line followed by another ">" line continues it).
    if (/^\s*>\s?/.test(line)) {
      const quoted: string[] = [];
      while (i < lines.length) {
        if (/^\s*>\s?/.test(lines[i])) {
          quoted.push(lines[i].replace(/^\s*>\s?/, ""));
          i++;
          continue;
        }
        if (isBlankLine(lines[i]) && /^\s*>\s?/.test(lines[i + 1] ?? "")) {
          quoted.push("");
          i++;
          continue;
        }
        break;
      }
      const paragraphs = quoted.join("\n").split(/\n\s*\n/).filter((p) => p.trim() !== "");
      out.push(
        `<blockquote>${
          paragraphs.map((p) => `<p>${renderInline(p.trim())}</p>`).join("")
        }</blockquote>`,
      );
      continue;
    }

    // Lists.
    if (isListItemLine(line)) {
      const ordered = /^\s*\d+\.\s+/.test(line);
      const { items, next } = parseListItems(lines, i, indentOf(line));
      out.push(renderList(items, ordered));
      i = next;
      continue;
    }

    // Paragraph: gather until blank line or next block construct.
    const para: string[] = [line];
    i++;
    while (
      i < lines.length && !isBlankLine(lines[i]) &&
      !/^```/.test(lines[i]) && !/^#{1,6}\s+/.test(lines[i]) &&
      !isListItemLine(lines[i]) && !/^\s*>\s?/.test(lines[i]) &&
      !/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(lines[i])
    ) {
      para.push(lines[i]);
      i++;
    }
    out.push(`<p>${renderInline(para.join("\n").trim())}</p>`);
  }

  return out.join("");
}

/** Convert a Markdown string to Leantime-compatible rich HTML. */
export function markdownToHtml(markdown: string): string {
  return parseLines(markdown.replace(/\r\n?/g, "\n").split("\n"));
}
