//! Inline rendering: escape, code spans, links, emphasis — the `render_inline`
//! pass shared by every block type.

/// Escape `& < > "` — raw HTML in Markdown sources always renders as text.
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn is_valid_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

pub(super) fn is_space(c: char) -> bool {
    c.is_whitespace()
}

/// `[label](url)` — label: 1+ chars, no `]`/newline; url: 1+ chars, no `)`/whitespace.
/// `![…](…)` is left untouched (images are not supported).
fn replace_links(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' && (i == 0 || chars[i - 1] != '!') {
            let mut j = i + 1;
            let mut label = String::new();
            while j < chars.len() && chars[j] != ']' && chars[j] != '\n' {
                label.push(chars[j]);
                j += 1;
            }
            if j < chars.len() && chars[j] == ']' && j + 1 < chars.len() && chars[j + 1] == '(' {
                let mut k = j + 2;
                let mut url = String::new();
                while k < chars.len() && chars[k] != ')' && !is_space(chars[k]) {
                    url.push(chars[k]);
                    k += 1;
                }
                if k < chars.len()
                    && chars[k] == ')'
                    && !url.is_empty()
                    && !label.is_empty()
                    && is_valid_url(&url)
                {
                    result.push_str(&format!("<a href=\"{}\">{}</a>", url, label));
                    i = k + 1;
                    continue;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// `~~text~~` — opening followed by non-space, lazy inner ending non-space.
fn replace_strikethrough(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    'outer: while i < chars.len() {
        if chars[i] == '~' && i + 3 < chars.len() && chars[i + 1] == '~' && !is_space(chars[i + 2])
        {
            let inner_start = i + 2;
            let mut j = inner_start + 1;
            while j + 1 < chars.len() {
                if chars[j] == '~' && chars[j + 1] == '~' && !is_space(chars[j - 1]) {
                    let inner: String = chars[inner_start..j].iter().collect();
                    result.push_str(&format!("<s>{}</s>", inner));
                    i = j + 2;
                    continue 'outer;
                }
                j += 1;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// Emphasis pass: `(^|[\s(])(mark)(?=\S)…(mark)(?=$|[\s).,!?;:])`.
/// `inner_first_last`: (may not contain char, must end non-space). Bold allows any
/// inner chars (`**`/`__`); italic excludes `*` and newlines.
fn replace_emphasis(input: &str, marks: &[&str], tag: &str, forbid_inner: &[char]) -> String {
    let chars: Vec<char> = input.chars().collect();
    let close_follow_ok = |c: char| is_space(c) || ").,!?;:".contains(c);
    let mut result = String::new();
    let mut i = 0;
    'outer: while i < chars.len() {
        // pre condition: line start, whitespace, or `(`
        let pre_ok = i == 0 || is_space(chars[i - 1]) || chars[i - 1] == '(';
        if pre_ok {
            for mark in marks {
                let m: Vec<char> = mark.chars().collect();
                let ml = m.len();
                if i + ml > chars.len() || chars[i..i + ml] != m[..] {
                    continue;
                }
                let inner_start = i + ml;
                // (?=\S): char right after the opening mark must be non-space
                if inner_start >= chars.len() || is_space(chars[inner_start]) {
                    continue;
                }
                // lazy scan for the shortest valid inner
                let mut j = inner_start + 1;
                while j + ml <= chars.len() {
                    // candidate closing mark at j; inner = [inner_start, j)
                    if chars[j..j + ml] == m[..] {
                        let inner_end = j;
                        // inner must end with non-space
                        if inner_end > inner_start && !is_space(chars[inner_end - 1]) {
                            let inner: String = chars[inner_start..inner_end].iter().collect();
                            let forbidden = inner.chars().any(|c| forbid_inner.contains(&c));
                            let after = inner_end + ml;
                            let follow_ok = after == chars.len()
                                || chars[after] == '\n'
                                || close_follow_ok(chars[after]);
                            if !forbidden && follow_ok {
                                result.push_str(&format!("<{}>{}</{}>", tag, inner, tag));
                                i = after;
                                continue 'outer;
                            }
                        }
                        // inner ending in space before the mark: keep extending is
                        // useless for THIS mark position — the lazy scan continues.
                    }
                    j += 1;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// Code spans: `(`{1,3})([^`\n]+?)\1` — the whole opening backtick run (≤3) is the
/// delimiter; content has no backticks/newlines; the closing run must be at least
/// as long (only its first `d` chars are consumed).
fn extract_code_spans(escaped: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = escaped.chars().collect();
    let mut spans: Vec<String> = Vec::new();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            let run_start = i;
            let mut run_end = i;
            while run_end < chars.len() && chars[run_end] == '`' {
                run_end += 1;
            }
            let run_len = run_end - run_start;
            if run_len <= 3 {
                // find content: chars up to the next backtick (no newlines)
                let mut j = run_end;
                let mut content = String::new();
                let mut found = false;
                while j < chars.len() {
                    if chars[j] == '`' {
                        found = true;
                        break;
                    }
                    if chars[j] == '\n' {
                        break;
                    }
                    content.push(chars[j]);
                    j += 1;
                }
                if found && !content.is_empty() {
                    // closing run must be ≥ run_len
                    let mut close_end = j;
                    while close_end < chars.len() && chars[close_end] == '`' {
                        close_end += 1;
                    }
                    if close_end - j >= run_len {
                        spans.push(format!("<code>{}</code>", content));
                        out.push_str(&format!("\x00{}\x00", spans.len() - 1));
                        i = j + run_len;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, spans)
}

/// Inline rendering: escape, then apply code spans, links, and marks.
pub(super) fn render_inline(text: &str) -> String {
    let (mut out, code_spans) = extract_code_spans(&escape_html(text));

    out = replace_links(&out);
    // Bold (boundary-anchored so snake_case and __dunder__ stay literal).
    out = replace_emphasis(&out, &["**", "__"], "strong", &[]);
    // Italic, same anchoring rules.
    out = replace_emphasis(&out, &["*", "_"], "em", &['*', '\n']);
    // Strikethrough.
    out = replace_strikethrough(&out);
    // Line breaks inside a block.
    out = out.replace('\n', "<br />");

    // Restore code spans.
    for (i, span) in code_spans.iter().enumerate() {
        out = out.replace(&format!("\x00{}\x00", i), span);
    }
    out
}
