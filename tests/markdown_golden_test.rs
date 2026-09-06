//! Golden corpus test: byte-for-byte parity with the TypeScript converter.
//! Regenerate with:
//!   deno run --allow-read --allow-write scripts/generate-golden-corpus.ts
//! (from the leantime-mcp repo). The corpus is committed alongside this test.

use serde_json::Value;

#[test]
fn golden_corpus_byte_parity() {
    let golden_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/md-golden.json");
    let raw = std::fs::read_to_string(&golden_path)
        .unwrap_or_else(|e| panic!("read {}: {}", golden_path.display(), e));
    let cases: Vec<Value> = serde_json::from_str(&raw).unwrap();

    let mut failures = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let input = case["input"].as_str().unwrap();
        let expected = case["expected"].as_str().unwrap();
        let actual = leantmcp::markdown::markdown_to_html(input);
        if actual != expected {
            failures.push(format!(
                "case \"{}\":\n  input:    {:?}\n  expected: {}\n  actual:   {}",
                name, input, expected, actual
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} golden cases diverge from the TS converter:\n\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n\n")
    );
}
