import { assertEquals } from "@std/assert";
import {
  IS_WINDOWS,
  doctorChecks,
  keyFileStatus,
  keySetCommand,
  keyShowCommand,
  keyTestCommand,
  maskKey,
  readKey,
  secretPath,
  writeKey,
  fileMode,
} from "../../src/keyring.ts";
import { setupConfig } from "../../src/main.ts";
import { createMockFetch } from "../helpers/mock-fetch.ts";

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

Deno.test("keyring — writeKey/readKey round-trip without trailing newline", async () => {
  await withTempHome(async () => {
    await writeKey(`${LONG_KEY}\n`);
    assertEquals(await readKey(), LONG_KEY);
    // File content has no trailing newline (opencode {file:} includes it verbatim)
    const raw = await Deno.readTextFile(secretPath());
    assertEquals(raw, LONG_KEY);
  });
});

Deno.test({
  name: "keyring — secret file is 0600, dir is 0700 (POSIX only)",
  ignore: IS_WINDOWS,
  fn: async () => {
    await withTempHome(async () => {
      await writeKey(LONG_KEY);
      assertEquals(await fileMode(secretPath()), "600");
      assertEquals(await fileMode(secretPath().replace("/api-key", "")), "700");
      const status = await keyFileStatus();
      assertEquals(status.exists, true);
      assertEquals(status.mode, "600");
    });
  },
});

Deno.test({
  name: "keyring — Windows reports ACL mode (null) without touching chmod",
  ignore: !IS_WINDOWS,
  fn: async () => {
    await withTempHome(async () => {
      await writeKey(LONG_KEY); // must not throw on Windows
      const status = await keyFileStatus();
      assertEquals(status.exists, true);
      assertEquals(status.mode, null);
    });
  },
});

Deno.test("keyring — maskKey shows 6 first / 4 last", () => {
  assertEquals(maskKey(LONG_KEY), "lt_h13…Fc3O");
  assertEquals(maskKey("short"), "sho…");
});

Deno.test("keyring — secretPath prefers HOME and falls back to USERPROFILE", async () => {
  await withTempHome(async () => {
    assertEquals(secretPath().startsWith("/tmp/"), true);
    assertEquals(secretPath().endsWith("/.config/leantime/api-key"), true);
  });
  const hadHome = Deno.env.get("HOME");
  const hadProfile = Deno.env.get("USERPROFILE");
  try {
    Deno.env.delete("HOME");
    Deno.env.set("USERPROFILE", "C:\\Users\\t");
    assertEquals(secretPath(), "C:\\Users\\t/.config/leantime/api-key");
  } finally {
    if (hadHome !== undefined) Deno.env.set("HOME", hadHome);
    if (hadProfile !== undefined) Deno.env.set("USERPROFILE", hadProfile);
  }
});

Deno.test("key set — explicit input writes the file and reports masked key", async () => {
  await withTempHome(async () => {
    const r = await keySetCommand(LONG_KEY);
    assertEquals(r.ok, true);
    assertEquals(r.message.includes("lt_h13…Fc3O"), true);
    assertEquals(await readKey(), LONG_KEY);
  });
});

Deno.test("key set — env var path works, argv-free", async () => {
  await withTempHome(async () => {
    Deno.env.set("LEANTIME_API_KEY", LONG_KEY);
    try {
      const r = await keySetCommand();
      assertEquals(r.ok, true);
      assertEquals(await readKey(), LONG_KEY);
    } finally {
      Deno.env.delete("LEANTIME_API_KEY");
    }
  });
});

Deno.test("key show — masked, no full key in output", async () => {
  await withTempHome(async () => {
    await writeKey(LONG_KEY);
    const r = await keyShowCommand();
    assertEquals(r.ok, true);
    assertEquals(r.message.includes(LONG_KEY), false);
    assertEquals(r.message.includes("lt_h13…Fc3O"), true);
  });
});

Deno.test("key test — validates against the instance via injected fetch", async () => {
  await withTempHome(async () => {
    await writeKey(LONG_KEY);
    Deno.env.set("LEANTIME_URL", "https://leantime.test");
    try {
      const { fetch: mockFetch } = createMockFetch();
      const ok = await keyTestCommand(mockFetch);
      assertEquals(ok.ok, true);
      assertEquals(ok.message.includes("2 users"), true);

      const failing = async () =>
        new Response(JSON.stringify({
          jsonrpc: "2.0",
          error: { code: -32000, message: "Invalid API Key" },
          id: 1,
        }), { status: 200 });
      const bad = await keyTestCommand(failing);
      assertEquals(bad.ok, false);
      assertEquals(bad.message.includes("Invalid API Key"), true);
    } finally {
      Deno.env.delete("LEANTIME_URL");
    }
  });
});

Deno.test("doctor — all checks pass with key file, URL and injected fetch", async () => {
  await withTempHome(async () => {
    await writeKey(LONG_KEY);
    Deno.env.set("LEANTIME_URL", "https://leantime.test");
    try {
      const { fetch: mockFetch } = createMockFetch();
      const results = await doctorChecks(mockFetch);
      const keyFile = results.find((c) => c.label === "key file")!;
      assertEquals(keyFile.status, "ok");
      const url = results.find((c) => c.label === "instance URL")!;
      assertEquals(url.status, "ok");
      const validation = results.find((c) => c.label === "key validation")!;
      assertEquals(validation.status, "ok");
      // No global config in the temp home → warn (not crash)
      const globalCfg = results.find((c) => c.label === "global opencode config")!;
      assertEquals(globalCfg.status, "warn");
    } finally {
      Deno.env.delete("LEANTIME_URL");
    }
  });
});

Deno.test("doctor — missing key file is a fail", async () => {
  await withTempHome(async () => {
    const results = await doctorChecks();
    const keyFile = results.find((c) => c.label === "key file")!;
    assertEquals(keyFile.status, "fail");
    assertEquals(keyFile.detail.includes("key set"), true);
  });
});

Deno.test("setup — writes a {file:} pointer when the secret matches", async () => {
  await withTempHome(async () => {
    await writeKey(LONG_KEY);
    const target = await setupConfig(true, {
      url: "https://leantime.test",
      apiKey: LONG_KEY,
    });
    const cfg = JSON.parse(await Deno.readTextFile(target));
    const env = cfg.mcp.leantime.environment;
    assertEquals(env.LEANTIME_API_KEY, `{file:${secretPath()}}`);
    assertEquals(env.LEANTIME_URL, "https://leantime.test");
    // No plaintext secret in the config
    const raw = await Deno.readTextFile(target);
    assertEquals(raw.includes(LONG_KEY), false);
  });
});

Deno.test({
  name: "setup — config file is chmod 600 (POSIX only)",
  ignore: IS_WINDOWS,
  fn: async () => {
    await withTempHome(async () => {
      await writeKey(LONG_KEY);
      const target = await setupConfig(true, {
        url: "https://leantime.test",
        apiKey: LONG_KEY,
      });
      assertEquals(await fileMode(target), "600");
    });
  },
});

Deno.test("setup — plaintext fallback when the secret file differs", async () => {
  await withTempHome(async () => {
    await writeKey("lt_a_completely_different_key_0000000000000000000000000");
    const target = await setupConfig(true, {
      url: "https://leantime.test",
      apiKey: LONG_KEY,
    });
    const cfg = JSON.parse(await Deno.readTextFile(target));
    assertEquals(cfg.mcp.leantime.environment.LEANTIME_API_KEY, LONG_KEY);
  });
});
