import { assertEquals } from "@std/assert";
import { markdownToHtml, escapeHtml } from "../../src/markdown.ts";

Deno.test("markdown — empty string yields empty html", () => {
  assertEquals(markdownToHtml(""), "");
});

Deno.test("markdown — single paragraph", () => {
  assertEquals(markdownToHtml("Simple text"), "<p>Simple text</p>");
});

Deno.test("markdown — two paragraphs separated by blank line", () => {
  assertEquals(markdownToHtml("First\n\nSecond"), "<p>First</p><p>Second</p>");
});

Deno.test("markdown — single newline inside paragraph becomes br", () => {
  assertEquals(markdownToHtml("line one\nline two"), "<p>line one<br />line two</p>");
});

Deno.test("markdown — headings h1 to h4", () => {
  assertEquals(markdownToHtml("# Title"), "<h1>Title</h1>");
  assertEquals(markdownToHtml("## Section"), "<h2>Section</h2>");
  assertEquals(markdownToHtml("### Sub"), "<h3>Sub</h3>");
  assertEquals(markdownToHtml("#### Deep"), "<h4>Deep</h4>");
});

Deno.test("markdown — h5 and h6 clamp to h4", () => {
  assertEquals(markdownToHtml("##### Five"), "<h4>Five</h4>");
  assertEquals(markdownToHtml("###### Six"), "<h4>Six</h4>");
});

Deno.test("markdown — bold", () => {
  assertEquals(markdownToHtml("**important**"), "<p><strong>important</strong></p>");
  assertEquals(markdownToHtml("__important__"), "<p><strong>important</strong></p>");
  assertEquals(
    markdownToHtml("text **bold** rest"),
    "<p>text <strong>bold</strong> rest</p>",
  );
});

Deno.test("markdown — italic", () => {
  assertEquals(markdownToHtml("*emphasis*"), "<p><em>emphasis</em></p>");
  assertEquals(markdownToHtml("text _ital_ rest"), "<p>text <em>ital</em> rest</p>");
});

Deno.test("markdown — strikethrough", () => {
  assertEquals(markdownToHtml("~~obsolete~~"), "<p><s>obsolete</s></p>");
});

Deno.test("markdown — snake_case identifiers stay literal, dunder follows CommonMark", () => {
  assertEquals(markdownToHtml("use some_var_name here"), "<p>use some_var_name here</p>");
  // Per CommonMark, a space-delimited __word__ IS bold ("__init__" alone is not, e.g. `__init__` in code spans).
  assertEquals(
    markdownToHtml("dunder __init__ method"),
    "<p>dunder <strong>init</strong> method</p>",
  );
});

Deno.test("markdown — inline code", () => {
  assertEquals(
    markdownToHtml("run `npm test` now"),
    "<p>run <code>npm test</code> now</p>",
  );
});

Deno.test("markdown — inline code protects markup", () => {
  assertEquals(
    markdownToHtml("``**not bold**`` x"),
    "<p><code>**not bold**</code> x</p>",
  );
});

Deno.test("markdown — links", () => {
  assertEquals(
    markdownToHtml("[docs](https://example.com)"),
    '<p><a href="https://example.com">docs</a></p>',
  );
  assertEquals(
    markdownToHtml("mail [me](mailto:a@b.co)"),
    '<p>mail <a href="mailto:a@b.co">me</a></p>',
  );
});

Deno.test("markdown — invalid link schemes stay literal", () => {
  assertEquals(
    markdownToHtml("[x](javascript:alert(1))"),
    "<p>[x](javascript:alert(1))</p>",
  );
});

Deno.test("markdown — image syntax stays literal", () => {
  assertEquals(
    markdownToHtml("![alt](https://example.com/i.png)"),
    "<p>![alt](https://example.com/i.png)</p>",
  );
});

Deno.test("markdown — unordered list", () => {
  assertEquals(
    markdownToHtml("- one\n- two\n- three"),
    "<ul><li><p>one</p></li><li><p>two</p></li><li><p>three</p></li></ul>",
  );
});

Deno.test("markdown — ordered list", () => {
  assertEquals(
    markdownToHtml("1. first\n2. second"),
    "<ol><li><p>first</p></li><li><p>second</p></li></ol>",
  );
});

Deno.test("markdown — nested list", () => {
  const html = markdownToHtml("- top\n  - child\n- top2");
  assertEquals(
    html,
    "<ul><li><p>top</p><ul><li><p>child</p></li></ul></li><li><p>top2</p></li></ul>",
  );
});

Deno.test("markdown — task list", () => {
  const html = markdownToHtml("- [ ] todo\n- [x] done");
  assertEquals(
    html,
    '<ul data-type="taskList"><li data-type="taskItem" data-checked="false"><p>todo</p></li>' +
      '<li data-type="taskItem" data-checked="true"><p>done</p></li></ul>',
  );
});

Deno.test("markdown — list vs horizontal rule", () => {
  assertEquals(markdownToHtml("---"), "<hr>");
  assertEquals(markdownToHtml("- item"), "<ul><li><p>item</p></li></ul>");
});

Deno.test("markdown — blockquote", () => {
  assertEquals(
    markdownToHtml("> quoted text"),
    "<blockquote><p>quoted text</p></blockquote>",
  );
  assertEquals(
    markdownToHtml("> para one\n\n> para two"),
    "<blockquote><p>para one</p><p>para two</p></blockquote>",
  );
});

Deno.test("markdown — fenced code block with language", () => {
  const html = markdownToHtml("```ts\nconst x = 1;\n```");
  assertEquals(
    html,
    '<pre><code class="language-ts">const x = 1;</code></pre>',
  );
});

Deno.test("markdown — fenced code block escapes html", () => {
  const html = markdownToHtml("```\n<script>alert(1)</script>\n```");
  assertEquals(
    html,
    "<pre><code>&lt;script&gt;alert(1)&lt;/script&gt;</code></pre>",
  );
});

Deno.test("markdown — raw html in text is escaped", () => {
  assertEquals(
    markdownToHtml("<b>not bold</b> & <i>tag</i>"),
    "<p>&lt;b&gt;not bold&lt;/b&gt; &amp; &lt;i&gt;tag&lt;/i&gt;</p>",
  );
});

Deno.test("markdown — mixed full document", () => {
  const md = [
    "## Objectif",
    "",
    "Livrer la **v1** avec `tests`.",
    "",
    "### Étapes",
    "",
    "1. setup",
    "2. build",
    "",
    "- [x] spec",
    "- [ ] merge",
    "",
    "> voir [docs](https://leantime.io)",
    "",
    "---",
    "",
    "Fin.",
  ].join("\n");
  assertEquals(
    markdownToHtml(md),
    "<h2>Objectif</h2>" +
      "<p>Livrer la <strong>v1</strong> avec <code>tests</code>.</p>" +
      "<h3>Étapes</h3>" +
      "<ol><li><p>setup</p></li><li><p>build</p></li></ol>" +
      '<ul data-type="taskList"><li data-type="taskItem" data-checked="true"><p>spec</p></li>' +
      '<li data-type="taskItem" data-checked="false"><p>merge</p></li></ul>' +
      '<blockquote><p>voir <a href="https://leantime.io">docs</a></p></blockquote>' +
      "<hr>" +
      "<p>Fin.</p>",
  );
});

Deno.test("markdown — crlf line endings normalized", () => {
  assertEquals(markdownToHtml("a\r\n\r\nb"), "<p>a</p><p>b</p>");
});

Deno.test("markdown — escapeHtml utility", () => {
  assertEquals(escapeHtml('<a href="x">&'), "&lt;a href=&quot;x&quot;&gt;&amp;");
});
