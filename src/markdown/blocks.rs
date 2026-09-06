//! Block-level parsing: headings, fences, quotes, lists (nested, task),
//! horizontal rules and paragraphs.

use super::inline::{escape_html, is_space, render_inline};

// ---------------------------------------------------------------------------
// Lists
// ---------------------------------------------------------------------------

struct ListItem {
    content: String,
    child_lines: Vec<String>,
    task: bool,
    checked: bool,
}

/// Skip the first `n` chars — char-boundary safe (JS `slice(n)` semantics
/// for BMP text; indentation may contain Unicode whitespace pasted from
/// word processors, and byte slicing would panic inside a multi-byte char).
fn skip_chars(line: &str, n: usize) -> &str {
    for (taken, (idx, _)) in line.char_indices().enumerate() {
        if taken == n {
            return &line[idx..];
        }
    }
    ""
}

/// `^(\s*)([-*]|\d+\.)\s+(.*)$` — indent counted in CHARS (the TS regex
/// measures UTF-16 units; chars match it for all BMP whitespace).
fn match_list_item(line: &str) -> Option<(usize, bool, String)> {
    let indent = line.chars().take_while(|c| c.is_whitespace()).count();
    let t = skip_chars(line, indent);
    let rest = if let Some(r) = t.strip_prefix("- ") {
        r
    } else if let Some(r) = t.strip_prefix("* ") {
        r
    } else if let Some(r) = t.strip_prefix("-\t") {
        r
    } else if let Some(r) = t.strip_prefix("*\t") {
        r
    } else {
        let digits: usize = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        let after = skip_chars(t, digits);
        if let Some(r) = after.strip_prefix(". ") {
            r
        } else {
            after.strip_prefix(".\t")?
        }
    };
    let ordered = t.starts_with(|c: char| c.is_ascii_digit());
    // rest already has the single separator space stripped; TS `\s+` consumed exactly
    // what we stripped plus any run — strip extra leading whitespace to match `\s+`.
    let content = rest.trim_start().to_string();
    if content.is_empty() && rest.is_empty() {
        return None;
    }
    Some((indent, ordered, content))
}

fn is_list_item_line(line: &str) -> bool {
    match_list_item(line).is_some()
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

fn parse_list_items(lines: &[&str], start: usize, indent: usize) -> (Vec<ListItem>, usize) {
    let mut items = Vec::new();
    let mut i = start;

    while i < lines.len() {
        let matched = match match_list_item(lines[i]) {
            Some(m) => m,
            None => break,
        };
        if matched.0 != indent {
            break;
        }
        let mut content = matched.2;
        let bullet_only = !matched.1;
        let mut task = false;
        let mut checked = false;
        if bullet_only {
            // `^\[([ xX])\]\s+(.*)$`
            let chars: Vec<char> = content.chars().collect();
            if chars.len() >= 4
                && chars[0] == '['
                && (chars[1] == 'x' || chars[1] == 'X' || chars[1] == ' ')
                && chars[2] == ']'
            {
                let mut k = 3;
                let mut ws = 0;
                while k < chars.len() && is_space(chars[k]) {
                    k += 1;
                    ws += 1;
                }
                if ws >= 1 {
                    task = true;
                    checked = chars[1] != ' ';
                    content = chars[k..].iter().collect();
                }
            }
        }
        i += 1;

        // Lazy continuation: plain non-blank lines extend the item text.
        while i < lines.len() {
            let l = lines[i];
            if l.trim().is_empty() || is_list_item_line(l) || indent_of(l) > indent {
                break;
            }
            content.push(' ');
            content.push_str(l.trim());
            i += 1;
        }

        // Deeper-indented lines become the item's nested block.
        let mut child_lines: Vec<String> = Vec::new();
        while i < lines.len() {
            let line = lines[i];
            if line.trim().is_empty() {
                let next = lines.get(i + 1).copied().unwrap_or("");
                if indent_of(next) > indent && !next.trim().is_empty() {
                    child_lines.push(String::new());
                    i += 1;
                    continue;
                }
                break;
            }
            if indent_of(line) > indent {
                // Strip min(indent + 2, prefix) CHARS — byte slicing here
                // panicked on multi-byte Unicode whitespace (NBSP et al.).
                let prefix = line.chars().take_while(|c| c.is_whitespace()).count();
                let strip = (indent + 2).min(prefix);
                child_lines.push(skip_chars(line, strip).to_string());
                i += 1;
                continue;
            }
            break;
        }
        while child_lines.last().map(|l| l.is_empty()).unwrap_or(false) {
            child_lines.pop();
        }

        items.push(ListItem {
            content,
            child_lines,
            task,
            checked,
        });
    }

    (items, i)
}

fn render_list(items: &[ListItem], ordered: bool, depth: usize) -> String {
    const MAX_LIST_DEPTH: usize = 32;
    let is_task = !ordered && items.iter().any(|item| item.task);
    let open = if is_task {
        "<ul data-type=\"taskList\">"
    } else if ordered {
        "<ol>"
    } else {
        "<ul>"
    };

    let body: String = items
        .iter()
        .map(|item| {
            let inner = render_inline(&item.content);
            let nested = if !item.child_lines.is_empty() {
                let refs: Vec<&str> = item.child_lines.iter().map(|s| s.as_str()).collect();
                if depth >= MAX_LIST_DEPTH {
                    // Render overflow nesting as plain text instead of recursing.
                    let text = refs.join(" ");
                    render_inline(&text)
                } else {
                    parse_lines_depth(&refs, depth + 1)
                }
            } else {
                String::new()
            };
            let tag = if is_task {
                format!(
                    "<li data-type=\"taskItem\" data-checked=\"{}\">",
                    item.checked
                )
            } else {
                "<li>".to_string()
            };
            format!("{}<p>{}</p>{}</li>", tag, inner, nested)
        })
        .collect();

    let close = if is_task || !ordered {
        "</ul>"
    } else {
        "</ol>"
    };
    format!("{}{}{}", open, body, close)
}

fn is_blank_line(line: &str) -> bool {
    line.trim().is_empty()
}

/// Split on blank-ish separator lines (regex `\n\s*\n`) — a line containing only
/// whitespace separates two paragraphs.
fn split_paragraphs(joined: &str) -> Vec<&str> {
    let mut paras: Vec<&str> = Vec::new();
    let bytes = joined.as_bytes();
    let mut para_start = 0;
    let mut i = 0;
    while i < joined.len() {
        if bytes[i] == b'\n' {
            // lookahead: whitespace-only until the next '\n'
            let mut j = i + 1;
            while j < joined.len() && (bytes[j] as char).is_ascii_whitespace() && bytes[j] != b'\n'
            {
                j += 1;
            }
            if j < joined.len() && bytes[j] == b'\n' && j > i {
                paras.push(&joined[para_start..i]);
                i = j + 1;
                para_start = i;
                continue;
            }
        }
        i += 1;
    }
    paras.push(&joined[para_start..]);
    paras.retain(|p| !p.trim().is_empty());
    paras
}

// ---------------------------------------------------------------------------
// Block parser
// ---------------------------------------------------------------------------

/// `^```(\w*)\s*$`
fn match_fence_open(line: &str) -> Option<String> {
    let rest = line.strip_prefix("```")?;
    let trimmed_end = rest.trim_end();
    let lang: String = trimmed_end
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if trimmed_end.len() != lang.len() {
        return None;
    } // non-word char in the middle
    Some(lang)
}

/// `^```\s*$`
fn is_fence_close(line: &str) -> bool {
    match line.strip_prefix("```") {
        Some(r) => r.trim().is_empty(),
        None => false,
    }
}

/// `^(#{1,6})\s+(.*)$`
fn match_heading(line: &str) -> Option<(usize, String)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    if rest.starts_with(' ') || rest.starts_with('\t') {
        Some((hashes, rest.trim().to_string()))
    } else {
        None
    }
}

/// `^\s*(-{3,}|\*{3,}|_{3,})\s*$`
fn is_hr(line: &str) -> bool {
    let t = line.trim();
    if t.len() < 3 {
        return false;
    }
    let first = t.chars().next().unwrap();
    (first == '-' || first == '*' || first == '_') && t.chars().all(|c| c == first)
}

/// `^\s*>\s?`
fn strip_quote_marker(line: &str) -> Option<String> {
    let indent = line.len() - line.trim_start().len();
    let after = &line[indent..];
    let after = after.strip_prefix('>')?;
    let after = after.strip_prefix(' ').unwrap_or(after);
    Some(after.to_string())
}

/// Block-level parser over a list of lines. `depth` guards against stack
/// exhaustion via deeply nested lists (recursion in `render_list`).
fn parse_lines(lines: &[&str]) -> String {
    parse_lines_depth(lines, 0)
}

fn parse_lines_depth(lines: &[&str], depth: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if is_blank_line(line) {
            i += 1;
            continue;
        }

        // Fenced code block.
        if let Some(lang) = match_fence_open(line) {
            let mut code: Vec<&str> = Vec::new();
            i += 1;
            while i < lines.len() && !is_fence_close(lines[i]) {
                code.push(lines[i]);
                i += 1;
            }
            i += 1; // closing fence (or EOF)
            let cls = if lang.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", lang)
            };
            out.push(format!(
                "<pre><code{}>{}</code></pre>",
                cls,
                escape_html(&code.join("\n"))
            ));
            continue;
        }

        // ATX heading (# to ####, deeper levels clamp to h4).
        if let Some((hashes, text)) = match_heading(line) {
            let level = hashes.min(4);
            out.push(format!("<h{}>{}</h{}>", level, render_inline(&text), level));
            i += 1;
            continue;
        }

        // Horizontal rule.
        if is_hr(line) {
            out.push("<hr>".to_string());
            i += 1;
            continue;
        }

        // Blockquote (a blank line followed by another ">" line continues it).
        if strip_quote_marker(line).is_some() {
            let mut quoted: Vec<String> = Vec::new();
            while i < lines.len() {
                if let Some(q) = strip_quote_marker(lines[i]) {
                    quoted.push(q);
                    i += 1;
                    continue;
                }
                if is_blank_line(lines[i])
                    && i + 1 < lines.len()
                    && strip_quote_marker(lines[i + 1]).is_some()
                {
                    quoted.push(String::new());
                    i += 1;
                    continue;
                }
                break;
            }
            // split on blank-ish separator lines (regex \n\s*\n)
            let joined = quoted.join("\n");
            let paras = split_paragraphs(&joined);
            let rendered: String = paras
                .iter()
                .map(|p| format!("<p>{}</p>", render_inline(p.trim())))
                .collect();
            out.push(format!("<blockquote>{}</blockquote>", rendered));
            continue;
        }

        // Lists.
        if let Some((indent, ordered, _)) = match_list_item(line) {
            let (items, next) = parse_list_items(lines, i, indent);
            out.push(render_list(&items, ordered, depth));
            i = next;
            continue;
        }

        // Paragraph: gather until blank line or next block construct.
        let mut para: Vec<&str> = vec![line];
        i += 1;
        while i < lines.len() {
            let l = lines[i];
            if is_blank_line(l)
                || l.starts_with("```")
                || match_heading(l).is_some()
                || is_list_item_line(l)
                || strip_quote_marker(l).is_some()
                || is_hr(l)
            {
                break;
            }
            para.push(l);
            i += 1;
        }
        let joined = para.join("\n");
        out.push(format!("<p>{}</p>", render_inline(joined.trim())));
    }

    out.join("")
}

/// Convert a Markdown string to Leantime-compatible rich HTML.
/// Inputs larger than 1 MB are rejected: the inline scanners are quadratic
/// in adversarial cases and no real ticket needs that much formatted text.
pub fn markdown_to_html(markdown: &str) -> String {
    const MAX_INPUT_BYTES: usize = 1024 * 1024;
    if markdown.len() > MAX_INPUT_BYTES {
        return format!(
            "<p>{}</p>",
            escape_html(&format!(
                "(Markdown input rejected: {} bytes exceeds the 1 MB limit)",
                markdown.len()
            ))
        );
    }
    let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    parse_lines(&lines)
}
