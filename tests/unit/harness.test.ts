import { assertEquals } from "@std/assert";
import {
  claudeDesktopConfigPath,
  codexConfigPath,
  cursorConfigPath,
  ensureKeyring,
  setupHarnessCommand,
} from "../../src/harness.ts";
import { readKey, readUrl, writeKey, writeUrl } from "../../src/keyring.ts";

const LONG_KEY = "lt_h13dyVu1uWRw5K3EZT79zeES7fWOTyUn_sN47bM5lXVVTmso4So67Cxewc1pEFc3O";

async function withTempHome<T>(fn: () => Promise<T>): Promise<T> {
  const original = Deno.env.get("HOME");
  const tmp = await Deno.makeTempDir();
  Deno.env.set("HOME", tmp);
  try {
    return await fn();
  } finally {
    if (original !== undefined) Deno.env.set("HOME", original);
    else Deno.env.delete("HOME");
    await Deno.remove(tmp, { recursive: true });
  }
}

async function withKeyring(): Promise<void> {
  await writeUrl("https://leantime.test");
  await writeKey(LONG_KEY);
}

Deno.test("harness — claude-desktop path per OS", async () => {
  await withTempHome(async () => {
    const p = claudeDesktopConfigPath();
    assertEquals(p.includes("claude_desktop_config.json"), true);
    if (Deno.build.os === "linux") assertEquals(p.includes("/.config/Claude/"), true);
    if (Deno.build.os === "darwin") {
      assertEquals(p.includes("Library/Application Support/Claude"), true);
    }
  });
});

Deno.test("harness — ensureKeyring fails cleanly when nothing is provided", async () => {
  await withTempHome(async () => {
    Deno.env.delete("LEANTIME_URL");
    Deno.env.delete("LEANTIME_API_KEY");
    const r = await ensureKeyring();
    assertEquals(r.ok, false);
    assertEquals(r.message.includes("LEANTIME_URL"), true);
  });
});

Deno.test("harness — ensureKeyring from env vars", async () => {
  await withTempHome(async () => {
    Deno.env.set("LEANTIME_URL", "https://from-env.leantime.test/");
    Deno.env.set("LEANTIME_API_KEY", LONG_KEY);
    try {
      const r = await ensureKeyring();
      assertEquals(r.ok, true, r.message);
      assertEquals(await readUrl(), "https://from-env.leantime.test");
      assertEquals(await readKey(), LONG_KEY);
    } finally {
      Deno.env.delete("LEANTIME_URL");
      Deno.env.delete("LEANTIME_API_KEY");
    }
  });
});

Deno.test("harness — cursor setup writes a bare command, no secrets, merges", async () => {
  await withTempHome(async () => {
    await withKeyring();
    // pre-existing config with another server
    await Deno.mkdir(`${Deno.env.get("HOME")}/.cursor`, { recursive: true });
    await Deno.writeTextFile(
      cursorConfigPath(),
      JSON.stringify({ mcpServers: { other: { command: "other" } } }),
    );

    const r = await setupHarnessCommand("cursor");
    assertEquals(r.ok, true, r.message);
    const cfg = JSON.parse(await Deno.readTextFile(cursorConfigPath()));
    assertEquals(typeof cfg.mcpServers.leantime.command, "string");
    assertEquals("env" in cfg.mcpServers.leantime, false, "no env block expected");
    assertEquals(JSON.stringify(cfg).includes("lt_"), false, "no plaintext key");
    assertEquals(cfg.mcpServers.other.command, "other", "existing servers preserved");
    assertEquals(r.message.includes("no secrets"), true);
  });
});

Deno.test("harness — claude-code writes ./.mcp.json in cwd", async () => {
  await withTempHome(async () => {
    await withKeyring();
    const cwd = Deno.cwd();
    const tmp = await Deno.makeTempDir();
    Deno.chdir(tmp);
    try {
      const r = await setupHarnessCommand("claude-code");
      assertEquals(r.ok, true, r.message);
      const cfg = JSON.parse(await Deno.readTextFile(`${tmp}/.mcp.json`));
      assertEquals(typeof cfg.mcpServers.leantime.command, "string");
      assertEquals("env" in cfg.mcpServers.leantime, false);
      assertEquals(r.message.includes("claude mcp add"), true, "user-scope hint shown");
    } finally {
      Deno.chdir(cwd);
      await Deno.remove(tmp, { recursive: true });
    }
  });
});

Deno.test("harness — codex appends TOML once, skips when present", async () => {
  await withTempHome(async () => {
    await withKeyring();
    const first = await setupHarnessCommand("codex");
    assertEquals(first.ok, true, first.message);
    const toml = await Deno.readTextFile(codexConfigPath());
    assertEquals(toml.includes("[mcp_servers.leantime]"), true);
    assertEquals(toml.includes("lt_"), false, "no plaintext key in TOML");

    // Idempotence guard
    const second = await setupHarnessCommand("codex");
    assertEquals(second.ok, true);
    assertEquals(second.message.includes("left untouched"), true);
    const toml2 = await Deno.readTextFile(codexConfigPath());
    assertEquals(
      (toml2.match(/\[mcp_servers\.leantime\]/g) ?? []).length,
      1,
      "section must not be duplicated",
    );
  });
});

Deno.test("harness — setup fails when keyring is missing and nothing provided", async () => {
  await withTempHome(async () => {
    Deno.env.delete("LEANTIME_URL");
    Deno.env.delete("LEANTIME_API_KEY");
    const r = await setupHarnessCommand("cursor");
    assertEquals(r.ok, false);
    assertEquals(r.message.includes("url set"), true);
    // no config written
    let exists = true;
    try {
      await Deno.readTextFile(cursorConfigPath());
    } catch {
      exists = false;
    }
    assertEquals(exists, false);
  });
});
