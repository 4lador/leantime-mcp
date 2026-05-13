import { assertEquals, assertThrows } from "@std/assert";
import { readJsonFile, writeJsonFile, buildMergedConfig, getMcpCommand, SetupError } from "../../src/main.ts";

Deno.test("readJsonFile — reads valid JSON", async () => {
  const path = await Deno.makeTempFile({ suffix: ".json" });
  await Deno.writeTextFile(path, JSON.stringify({ hello: "world" }));
  try {
    const result = await readJsonFile(path);
    assertEquals(result, { hello: "world" });
  } finally {
    await Deno.remove(path);
  }
});

Deno.test("readJsonFile — returns empty object for missing file", async () => {
  const result = await readJsonFile("/tmp/does-not-exist-12345.json");
  assertEquals(result, {});
});

Deno.test("readJsonFile — returns empty object for invalid JSON", async () => {
  const path = await Deno.makeTempFile({ suffix: ".json" });
  await Deno.writeTextFile(path, "not json {{{{");
  try {
    const result = await readJsonFile(path);
    assertEquals(result, {});
  } finally {
    await Deno.remove(path);
  }
});

Deno.test("writeJsonFile — writes JSON and creates parent dirs", async () => {
  const dir = await Deno.makeTempDir();
  const path = `${dir}/sub/dir/test.json`;
  await writeJsonFile(path, { a: 1 });
  try {
    const content = await Deno.readTextFile(path);
    assertEquals(JSON.parse(content), { a: 1 });
  } finally {
    await Deno.remove(dir, { recursive: true });
  }
});

Deno.test("writeJsonFile — overwrites existing file", async () => {
  const path = await Deno.makeTempFile({ suffix: ".json" });
  await writeJsonFile(path, { v: 1 });
  await writeJsonFile(path, { v: 2 });
  try {
    const content = await Deno.readTextFile(path);
    assertEquals(JSON.parse(content), { v: 2 });
  } finally {
    await Deno.remove(path);
  }
});

Deno.test("buildMergedConfig — creates config from scratch", () => {
  const result = buildMergedConfig({}, ["/usr/bin/leantmcp"], "https://test.leantime.io", "lt_key");
  const mcp = result.mcp as Record<string, Record<string, unknown>>;
  assertEquals(mcp.leantime.type, "local");
  assertEquals(mcp.leantime.command, ["/usr/bin/leantmcp"]);
  assertEquals((mcp.leantime.environment as Record<string, unknown>).LEANTIME_URL, "https://test.leantime.io");
  assertEquals((mcp.leantime.environment as Record<string, unknown>).LEANTIME_API_KEY, "lt_key");
});

Deno.test("buildMergedConfig — preserves existing mcp servers", () => {
  const existing = {
    mcp: {
      otherServer: { type: "local", command: ["other"] },
    },
  };
  const result = buildMergedConfig(existing, ["/usr/bin/leantmcp"], "https://test.leantime.io", "lt_key");
  assertEquals((result.mcp as Record<string, unknown>).otherServer, { type: "local", command: ["other"] });
  const mcp = result.mcp as Record<string, Record<string, unknown>>;
  assertEquals(mcp.leantime.type, "local");
});

Deno.test("buildMergedConfig — overwrites existing leantime config", () => {
  const existing = {
    mcp: {
      leantime: { type: "local", command: ["old"], environment: { LEANTIME_URL: "https://old.com", LEANTIME_API_KEY: "old_key" } },
    },
  };
  const result = buildMergedConfig(existing, ["/usr/bin/leantmcp"], "https://new.com", "new_key");
  const env = ((result.mcp as Record<string, Record<string, unknown>>).leantime.environment as Record<string, unknown>);
  assertEquals(env.LEANTIME_URL, "https://new.com");
  assertEquals(env.LEANTIME_API_KEY, "new_key");
});

Deno.test("buildMergedConfig — preserves non-mcp keys", () => {
  const existing = { schema: "https://example.com", other: true };
  const result = buildMergedConfig(existing, ["/usr/bin/leantmcp"], "https://test.leantime.io", "lt_key");
  assertEquals(result.schema, "https://example.com");
  assertEquals(result.other, true);
});

Deno.test("buildMergedConfig — does not strip trailing slashes (setupConfig does that)", () => {
  const result = buildMergedConfig({}, ["/usr/bin/leantmcp"], "https://test.leantime.io///", "lt_key");
  const env = ((result.mcp as Record<string, Record<string, unknown>>).leantime.environment as Record<string, unknown>);
  assertEquals(env.LEANTIME_URL, "https://test.leantime.io///");
});

Deno.test("getMcpCommand — returns array", () => {
  const cmd = getMcpCommand();
  assertEquals(Array.isArray(cmd), true);
  assertEquals(cmd.length > 0, true);
});
