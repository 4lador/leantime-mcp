#[cfg(test)]
mod tests {
    use leantmcp::markdown::*;

    // ---- Headings ----

    #[test]
    fn heading_h1_to_h4() {
        assert_eq!(markdown_to_html("# Title"), "<h1>Title</h1>");
        assert_eq!(markdown_to_html("## Section"), "<h2>Section</h2>");
        assert_eq!(markdown_to_html("### Sub"), "<h3>Sub</h3>");
        assert_eq!(markdown_to_html("#### Deep"), "<h4>Deep</h4>");
    }

    #[test]
    fn heading_h5_h6_clamp_to_h4() {
        assert_eq!(markdown_to_html("##### Five"), "<h4>Five</h4>");
        assert_eq!(markdown_to_html("###### Six"), "<h4>Six</h4>");
    }

    // ---- Paragraphs ----

    #[test]
    fn single_paragraph() {
        assert_eq!(markdown_to_html("Simple text"), "<p>Simple text</p>");
    }

    #[test]
    fn two_paragraphs() {
        assert_eq!(
            markdown_to_html("First\n\nSecond"),
            "<p>First</p><p>Second</p>"
        );
    }

    #[test]
    fn single_newline_becomes_br() {
        assert_eq!(
            markdown_to_html("line one\nline two"),
            "<p>line one<br />line two</p>"
        );
    }

    // ---- Emphasis ----

    #[test]
    fn bold() {
        assert_eq!(
            markdown_to_html("**important**"),
            "<p><strong>important</strong></p>"
        );
    }

    #[test]
    fn italic() {
        assert_eq!(markdown_to_html("*emphasis*"), "<p><em>emphasis</em></p>");
    }

    #[test]
    fn strikethrough() {
        assert_eq!(markdown_to_html("~~obsolete~~"), "<p><s>obsolete</s></p>");
    }

    #[test]
    fn snake_case_stays_literal() {
        assert_eq!(
            markdown_to_html("use some_var_name here"),
            "<p>use some_var_name here</p>"
        );
    }

    // ---- Code ----

    #[test]
    fn inline_code() {
        assert_eq!(
            markdown_to_html("run `npm test` now"),
            "<p>run <code>npm test</code> now</p>"
        );
    }

    #[test]
    fn inline_code_protects_markup() {
        assert_eq!(
            markdown_to_html("``**not bold**`` x"),
            "<p><code>**not bold**</code> x</p>"
        );
    }

    // ---- Links ----

    #[test]
    fn links() {
        assert_eq!(
            markdown_to_html("[docs](https://example.com)"),
            "<p><a href=\"https://example.com\">docs</a></p>"
        );
    }

    #[test]
    fn invalid_link_scheme_stays_literal() {
        assert_eq!(
            markdown_to_html("[x](javascript:alert(1))"),
            "<p>[x](javascript:alert(1))</p>"
        );
    }

    #[test]
    fn image_syntax_stays_literal() {
        assert_eq!(
            markdown_to_html("![alt](https://example.com/i.png)"),
            "<p>![alt](https://example.com/i.png)</p>"
        );
    }

    // ---- Lists ----

    #[test]
    fn unordered_list() {
        assert_eq!(
            markdown_to_html("- one\n- two\n- three"),
            "<ul><li><p>one</p></li><li><p>two</p></li><li><p>three</p></li></ul>"
        );
    }

    #[test]
    fn ordered_list() {
        assert_eq!(
            markdown_to_html("1. first\n2. second"),
            "<ol><li><p>first</p></li><li><p>second</p></li></ol>"
        );
    }

    #[test]
    fn task_list() {
        let html = markdown_to_html("- [ ] todo\n- [x] done");
        assert!(html.contains("<ul data-type=\"taskList\">"));
        assert!(html.contains("data-checked=\"false\""));
        assert!(html.contains("data-checked=\"true\""));
    }

    // ---- Blockquote ----

    #[test]
    fn blockquote() {
        assert_eq!(
            markdown_to_html("> quoted text"),
            "<blockquote><p>quoted text</p></blockquote>"
        );
    }

    // ---- Code blocks ----

    #[test]
    fn fenced_code_with_language() {
        assert_eq!(
            markdown_to_html("```ts\nconst x = 1;\n```"),
            "<pre><code class=\"language-ts\">const x = 1;</code></pre>"
        );
    }

    #[test]
    fn fenced_code_escapes_html() {
        assert_eq!(
            markdown_to_html("```\n<script>alert(1)</script>\n```"),
            "<pre><code>&lt;script&gt;alert(1)&lt;/script&gt;</code></pre>"
        );
    }

    // ---- Horizontal rule ----

    #[test]
    fn horizontal_rule_vs_list() {
        assert_eq!(markdown_to_html("---"), "<hr>");
        assert_eq!(markdown_to_html("- item"), "<ul><li><p>item</p></li></ul>");
    }

    // ---- Escaping ----

    #[test]
    fn raw_html_escaped() {
        assert_eq!(
            markdown_to_html("<b>not bold</b> & <i>tag</i>"),
            "<p>&lt;b&gt;not bold&lt;/b&gt; &amp; &lt;i&gt;tag&lt;/i&gt;</p>"
        );
    }

    // ---- Integration ----

    #[test]
    fn mixed_full_document() {
        let md = "## Objectif\n\nLivrer la **v1** avec `tests`.\n\n### Étapes\n\n1. setup\n2. build\n\n- [x] spec\n- [ ] merge\n\n---\n\nFin.";
        let html = markdown_to_html(md);
        assert!(html.contains("<h2>Objectif</h2>"));
        assert!(html.contains("<strong>v1</strong>"));
        assert!(html.contains("<ol>"));
        assert!(html.contains("<ul data-type=\"taskList\">"));
        assert!(html.contains("<hr>"));
        assert!(html.contains("<p>Fin.</p>"));
    }

    #[test]
    fn crlf_normalized() {
        assert_eq!(markdown_to_html("a\r\n\r\nb"), "<p>a</p><p>b</p>");
    }
}
