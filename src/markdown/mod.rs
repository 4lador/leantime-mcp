//! Deterministic Markdown → HTML converter.
//! Faithful port of src/markdown.ts — produces exactly the HTML subset
//! understood by Leantime's TipTap editor. Raw HTML in source is always
//! escaped. Behavior is pinned byte-for-byte by a 73-case golden corpus
//! (`tests/fixtures/md-golden.json`).

mod blocks;
mod inline;

pub use blocks::markdown_to_html;
pub use inline::escape_html;
