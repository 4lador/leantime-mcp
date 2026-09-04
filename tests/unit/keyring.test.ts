import { assertEquals } from "@std/assert";
import {
  IS_WINDOWS,
  activeInstance,
  doctorChecks,
  instanceAddCommand,
  instanceListCommand,
  instanceNames,
  instanceRemoveCommand,
  instanceUrlPath,
  instanceUseCommand,
  keyFileStatus,
  keyRotateCommand,
  keySetCommand,
  keyShowCommand,
  keyTestCommand,
  maskKey,
  readDefaultInstance,
  readKey,
  readUrl,
  resolveServerEnv,
  resolveUrl,
  resolveUrlDetailed,
  resolvedSecretPath,
  resolvedInstanceUrlPath,
  secretPath,
  urlSetCommand,
  urlShowCommand,
  writeDefaultInstance,
  writeKey,
  writeUrl,
  fileMode,
} from "../../src/keyring.ts";
import { setupConfig } from "../../src/main.ts";
import { createMockFetch } from "../helpers/mock-fetch.ts";
import { RPC_ERROR, RPC_OK } from "../helpers/fixtures.ts";

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
  const original = Deno.env.get("HOME");
  const tmp = await Deno.makeTempDir();
  Deno.env.set("HOME", tmp);
  Deno.env.delete("LEANTIME_INSTANCE");
  try {
    await writeDefaultInstance("default");
    assertEquals(secretPath().startsWith(tmp), true);
    assertEquals(secretPath().endsWith("/instances/default/api-key"), true);
  } finally {
    if (original !== undefined) Deno.env.set("HOME", original);
    else Deno.env.delete("HOME");
    await Deno.remove(tmp, { recursive: true });
  }
  const hadHome = Deno.env.get("HOME");
  const hadProfile = Deno.env.get("USERPROFILE");
  try {
    Deno.env.delete("HOME");
    Deno.env.set("USERPROFILE", "C:\\Users\\t");
    assertEquals(secretPath(), "C:\\Users\\t/.config/leantime/instances/default/api-key");
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
    assertEquals(env.LEANTIME_URL, `{file:${instanceUrlPath()}}`);
    // No plaintext secret in the config
    const raw = await Deno.readTextFile(target);
    assertEquals(raw.includes(LONG_KEY), false);
    // URL persisted to the keyring dir (single source of truth)
    assertEquals(await readUrl(), "https://leantime.test");
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

// ---------------------------------------------------------------- key rotate

const CURRENT_KEY = "lt_testusername123456789012345678_oldsecret_000000000000000";
const MINTED_KEY = "lt_nEwUsEr12345678901234567890ab_pAsSwOrD12345678901234567890ab";

async function withRotateEnv(fn: () => Promise<void>): Promise<void> {
  await withTempHome(async () => {
    await writeKey(CURRENT_KEY);
    Deno.env.set("LEANTIME_URL", "https://leantime.test");
    try {
      await fn();
    } finally {
      Deno.env.delete("LEANTIME_URL");
    }
  });
}

Deno.test("key rotate — happy path: source api, same role, live test before write", async () => {
  await withRotateEnv(async () => {
    const { fetch: mockFetch, calls } = createMockFetch();
    const r = await keyRotateCommand({ fetchFn: mockFetch });
    assertEquals(r.ok, true, r.message);
    // Stored key replaced by the minted one
    assertEquals(await readKey(), MINTED_KEY);
    // createAPIKey called with source:'api' + preserved role
    const create = calls.find((c) => c.method === "leantime.rpc.Api.createAPIKey")!;
    const values = create.params.values as Record<string, unknown>;
    assertEquals(values.source, "api");
    assertEquals(values.role, "20");
    assertEquals(String(values.firstname).startsWith("MCP-rotated-"), true);
    // Project assignments copied from the old key onto the new one
    const rel = calls.find((c) => c.method === "leantime.rpc.Projects.editUserProjectRelations")!;
    assertEquals(rel.params.id, "5"); // new key user id from createAPIKey
    assertEquals(rel.params.projects, ["3", "4"]); // old key's projects (fixture)
    assertEquals(r.message.includes("WARNING: could not copy project"), false);
    // live verification happened with the NEW key before writeKey
    const verify = calls.find((c) => c.method === "leantime.rpc.users.getAll")!;
    assertEquals(verify.method, "leantime.rpc.users.getAll");
    // message includes masked old key for UI deletion
    assertEquals(r.message.includes(maskKey(CURRENT_KEY)), true);
  });
});

Deno.test("key rotate — project relation copy failure warns but rotates", async () => {
  await withRotateEnv(async () => {
    const { fetch: mockFetch } = createMockFetch({
      "leantime.rpc.Projects.editUserProjectRelations": () =>
        RPC_ERROR(-32000, "Not authorized"),
    });
    const r = await keyRotateCommand({ fetchFn: mockFetch });
    assertEquals(r.ok, true);
    assertEquals(r.message.includes("WARNING: could not copy project"), true);
    assertEquals(await readKey(), MINTED_KEY);
  });
});

Deno.test("key rotate — custom --name", async () => {
  await withRotateEnv(async () => {
    const { fetch: mockFetch, calls } = createMockFetch();
    const r = await keyRotateCommand({ name: "mcp-prod", fetchFn: mockFetch });
    assertEquals(r.ok, true);
    const create = calls.find((c) => c.method === "leantime.rpc.Api.createAPIKey")!;
    assertEquals((create.params.values as Record<string, unknown>).firstname, "mcp-prod");
  });
});

Deno.test("key rotate — live verification failure leaves the keyring intact", async () => {
  await withRotateEnv(async () => {
    const { fetch: mockFetch } = createMockFetch({
      "leantime.rpc.users.getAll": () =>
        RPC_ERROR(-32000, "Invalid API Key"),
    });
    const r = await keyRotateCommand({ fetchFn: mockFetch });
    assertEquals(r.ok, false);
    assertEquals(r.message.includes("rotation aborted"), true);
    assertEquals(r.message.includes("untouched"), true);
    // The previous key is untouched
    assertEquals(await readKey(), CURRENT_KEY);
  });
});

Deno.test("key rotate — current key not found in API key list aborts", async () => {
  await withRotateEnv(async () => {
    const { fetch: mockFetch, calls } = createMockFetch({
      "leantime.rpc.Api.getAPIKeys": () => RPC_OK([]),
    });
    const r = await keyRotateCommand({ fetchFn: mockFetch });
    assertEquals(r.ok, false);
    assertEquals(r.message.includes("aborted"), true);
    assertEquals(await readKey(), CURRENT_KEY);
    assertEquals(calls.some((c) => c.method === "leantime.rpc.Api.createAPIKey"), false);
  });
});

Deno.test("key rotate — no stored key is a clean error", async () => {
  await withTempHome(async () => {
    const r = await keyRotateCommand();
    assertEquals(r.ok, false);
    assertEquals(r.message.includes("key set"), true);
  });
});

// ---------------------------------------------------------------- v1.4.2: url store & unified resolution

Deno.test("url — writeUrl/readUrl round-trip, slashes and newline stripped", async () => {
  await withTempHome(async () => {
    assertEquals(await writeUrl("https://leantime.test///\n"), instanceUrlPath());
    assertEquals(await readUrl(), "https://leantime.test");
    assertEquals(await Deno.readTextFile(instanceUrlPath()), "https://leantime.test");
  });
});

Deno.test("url — resolveUrlDetailed order: env > keyring file > legacy config", async () => {
  const hadEnv = Deno.env.get("LEANTIME_URL");
  await withTempHome(async () => {
    // 1. keyring file
    await writeUrl("https://from-file.leantime.test");
    const fromFile = (await resolveUrlDetailed())!;
    assertEquals(fromFile.url, "https://from-file.leantime.test");
    assertEquals(fromFile.source, "keyring");
    // 2. env wins
    Deno.env.set("LEANTIME_URL", "https://from-env.leantime.test");
    try {
      const fromEnv = (await resolveUrlDetailed())!;
      assertEquals(fromEnv.url, "https://from-env.leantime.test");
      assertEquals(fromEnv.source, "env");
    } finally {
      if (hadEnv !== undefined) Deno.env.set("LEANTIME_URL", hadEnv);
      else Deno.env.delete("LEANTIME_URL");
    }
  });
});

Deno.test("url — legacy config fallback works, pointer-only config is ignored", async () => {
  const hadEnv = Deno.env.get("LEANTIME_URL");
  if (hadEnv !== undefined) Deno.env.delete("LEANTIME_URL");
  const original = Deno.env.get("HOME");
  const tmp = await Deno.makeTempDir();
  Deno.env.set("HOME", tmp);
  try {
    // No keyring file, config with plaintext URL → source config
    await Deno.mkdir(`${tmp}/.opencode`, { recursive: true });
    await Deno.writeTextFile(
      `${tmp}/.opencode/opencode.json`,
      JSON.stringify({ mcp: { leantime: { environment: { LEANTIME_URL: "https://legacy.leantime.test/" } } } }),
    );
    const legacy = (await resolveUrlDetailed())!;
    assertEquals(legacy.url, "https://legacy.leantime.test");
    assertEquals(legacy.source, "config");

    // Config holding only a pointer → useless as URL, resolution returns null
    await Deno.writeTextFile(
      `${tmp}/.opencode/opencode.json`,
      JSON.stringify({ mcp: { leantime: { environment: { LEANTIME_URL: "{file:/some/path}" } } } }),
    );
    assertEquals(await resolveUrlDetailed(), null);
    assertEquals(await resolveUrl(), null);
  } finally {
    if (original !== undefined) Deno.env.set("HOME", original);
    else Deno.env.delete("HOME");
    if (hadEnv !== undefined) Deno.env.set("LEANTIME_URL", hadEnv);
    await Deno.remove(tmp, { recursive: true });
  }
});

Deno.test("resolveServerEnv — env wins, then keyring fallback, then explicit missing", async () => {
  const hadUrl = Deno.env.get("LEANTIME_URL");
  const hadKey = Deno.env.get("LEANTIME_API_KEY");
  Deno.env.delete("LEANTIME_URL");
  Deno.env.delete("LEANTIME_API_KEY");
  try {
    await withTempHome(async () => {
      // Nothing anywhere
      const none = await resolveServerEnv();
      assertEquals(none.ok, false);
      assertEquals((none as { missing: string[] }).missing.sort(), ["LEANTIME_API_KEY", "LEANTIME_URL"]);

      // Keyring fallback for both
      await writeUrl("https://keyring.leantime.test");
      await writeKey(LONG_KEY);
      const fromKeyring = await resolveServerEnv();
      assertEquals(fromKeyring.ok, true);
      assertEquals((fromKeyring as { url: string }).url, "https://keyring.leantime.test");
      assertEquals((fromKeyring as { apiKey: string }).apiKey, LONG_KEY);

      // Env override wins
      Deno.env.set("LEANTIME_URL", "https://override.leantime.test");
      Deno.env.set("LEANTIME_API_KEY", "lt_env_override");
      try {
        const fromEnv = await resolveServerEnv();
        assertEquals((fromEnv as { url: string }).url, "https://override.leantime.test");
        assertEquals((fromEnv as { apiKey: string }).apiKey, "lt_env_override");
      } finally {
        Deno.env.delete("LEANTIME_URL");
        Deno.env.delete("LEANTIME_API_KEY");
      }
    });
  } finally {
    if (hadUrl !== undefined) Deno.env.set("LEANTIME_URL", hadUrl);
    if (hadKey !== undefined) Deno.env.set("LEANTIME_API_KEY", hadKey);
  }
});

Deno.test("url set — argv input, live key verification, pointer-follow note", async () => {
  await withTempHome(async () => {
    await writeKey(LONG_KEY);
    Deno.env.delete("LEANTIME_URL");
    const { fetch: mockFetch } = createMockFetch();
    const r = await urlSetCommand("https://new-instance.leantime.test", mockFetch);
    assertEquals(r.ok, true, r.message);
    assertEquals(await readUrl(), "https://new-instance.leantime.test");
    assertEquals(r.message.includes("verified against the new instance"), true);
    assertEquals(r.message.includes("follow automatically"), true);
  });
});

Deno.test("url set — failed key verification warns but still stores", async () => {
  await withTempHome(async () => {
    await writeKey(LONG_KEY);
    const failing = async () =>
      new Response(JSON.stringify({
        jsonrpc: "2.0",
        error: { code: -32000, message: "Invalid API Key" },
        id: 1,
      }), { status: 200 });
    const r = await urlSetCommand("https://other-instance.leantime.test", failing);
    assertEquals(r.ok, true);
    assertEquals(r.message.includes("WARNING"), true);
    assertEquals(r.message.includes("key set"), true);
  });
});

Deno.test("url set — no input is a clean error", async () => {
  await withTempHome(async () => {
    const hadEnv = Deno.env.get("LEANTIME_URL");
    if (hadEnv !== undefined) Deno.env.delete("LEANTIME_URL");
    try {
      const r = await urlSetCommand();
      assertEquals(r.ok, false);
    } finally {
      if (hadEnv !== undefined) Deno.env.set("LEANTIME_URL", hadEnv);
    }
  });
});

Deno.test("url show — displays resolved URL with its source", async () => {
  await withTempHome(async () => {
    await writeUrl("https://shown.leantime.test");
    const hadEnv = Deno.env.get("LEANTIME_URL");
    if (hadEnv !== undefined) Deno.env.delete("LEANTIME_URL");
    try {
      const r = await urlShowCommand();
      assertEquals(r.ok, true);
      assertEquals(r.message.includes("https://shown.leantime.test"), true);
      assertEquals(r.message.includes("keyring"), true);
    } finally {
      if (hadEnv !== undefined) Deno.env.set("LEANTIME_URL", hadEnv);
    }
  });
});

Deno.test("doctor — .env drift is detected (mismatch vs duplicate)", async () => {
  const originalCwd = Deno.cwd();
  const tmp = await Deno.makeTempDir();
  const originalHome = Deno.env.get("HOME");
  const hadUrl = Deno.env.get("LEANTIME_URL");
  Deno.env.set("HOME", tmp);
  Deno.env.delete("LEANTIME_URL");
  try {
    await writeKey(LONG_KEY);
    await writeUrl("https://leantime.test");

    // Stale copy in the cwd .env
    Deno.chdir(tmp);
    await Deno.writeTextFile(`${tmp}/.env`, "LEANTIME_API_KEY=lt_a_stale_different_key\n");
    const drift = await doctorChecks();
    const staleCheck = drift.find((c) => c.label === ".env key copy")!;
    assertEquals(staleCheck.status, "warn");
    assertEquals(staleCheck.detail.includes("DIFFERENT"), true);

    // Exact duplicate
    await Deno.writeTextFile(`${tmp}/.env`, `LEANTIME_API_KEY=${LONG_KEY}\n`);
    const dup = await doctorChecks();
    const dupCheck = dup.find((c) => c.label === ".env key copy")!;
    assertEquals(dupCheck.status, "warn");
    assertEquals(dupCheck.detail.includes("duplicate"), true);

    // No .env at all — no check fired
    await Deno.remove(`${tmp}/.env`);
    const clean = await doctorChecks();
    assertEquals(clean.find((c) => c.label === ".env key copy"), undefined);
  } finally {
    Deno.chdir(originalCwd);
    if (originalHome !== undefined) Deno.env.set("HOME", originalHome);
    else Deno.env.delete("HOME");
    if (hadUrl !== undefined) Deno.env.set("LEANTIME_URL", hadUrl);
    await Deno.remove(tmp, { recursive: true });
  }
});

// ---------------------------------------------------------------- v1.5.0: doctor plaintext detection in harness configs

Deno.test("doctor — plaintext key in a harness config is flagged", async () => {
  const originalCwd = Deno.cwd();
  const tmp = await Deno.makeTempDir();
  const originalHome = Deno.env.get("HOME");
  Deno.env.set("HOME", tmp);
  Deno.env.delete("LEANTIME_URL");
  try {
    await writeKey(LONG_KEY);
    await writeUrl("https://leantime.test");
    Deno.chdir(tmp);
    // A claude-code project config with a plaintext key (the old bad practice)
    await Deno.writeTextFile(
      `${tmp}/.mcp.json`,
      JSON.stringify({
        mcpServers: { leantime: { command: "leantmcp", env: { LEANTIME_API_KEY: LONG_KEY } } },
      }),
    );
    const results = await doctorChecks();
    const flagged = results.find((c) => c.label.startsWith("plaintext key in claude-code"));
    assertEquals(flagged !== undefined, true, "plaintext key not detected");
    assertEquals(flagged!.status, "warn");
    assertEquals(flagged!.detail.includes(".mcp.json"), true);
    assertEquals(flagged!.detail.includes("setup"), true);
  } finally {
    Deno.chdir(originalCwd);
    if (originalHome !== undefined) Deno.env.set("HOME", originalHome);
    else Deno.env.delete("HOME");
    await Deno.remove(tmp, { recursive: true });
  }
});

// ---------------------------------------------------------------- v1.6.0: instance profiles

async function clearInstanceEnv(): Promise<void> {
  Deno.env.delete("LEANTIME_INSTANCE");
}

Deno.test("instances — paths follow LEANTIME_INSTANCE", async () => {
  await withTempHome(async () => {
    await clearInstanceEnv();
    await writeDefaultInstance("prod");
    // Async-resolved paths follow the default file
    assertEquals((await resolvedSecretPath()).endsWith("/instances/prod/api-key"), true);
    assertEquals((await resolvedInstanceUrlPath()).endsWith("/instances/prod/instance-url"), true);
    // Sync display paths follow the env (or "default" when unset)
    Deno.env.set("LEANTIME_INSTANCE", "staging");
    try {
      assertEquals(secretPath().endsWith("/instances/staging/api-key"), true);
      assertEquals(instanceUrlPath().endsWith("/instances/staging/instance-url"), true);
      assertEquals((await resolvedSecretPath()).endsWith("/instances/staging/api-key"), true);
    } finally {
      await clearInstanceEnv();
    }
  });
});

Deno.test("instances — add/list/remove round-trip via env vars", async () => {
  await withTempHome(async () => {
    await clearInstanceEnv();
    Deno.env.set("LEANTIME_URL", "https://staging.leantime.test");
    Deno.env.set("LEANTIME_API_KEY", LONG_KEY);
    try {
      const add = await instanceAddCommand("staging");
      assertEquals(add.ok, true, add.message);
      assertEquals((await instanceNames()).includes("staging"), true);

      const list = await instanceListCommand();
      assertEquals(list.ok, true);
      assertEquals(list.message.includes("staging"), true);
      assertEquals(list.message.includes(maskKey(LONG_KEY)), true);

      const dup = await instanceAddCommand("staging");
      assertEquals(dup.ok, false);

      const rm = await instanceRemoveCommand("staging");
      assertEquals(rm.ok, true, rm.message);
      assertEquals((await instanceNames()).includes("staging"), false);
      const rmAgain = await instanceRemoveCommand("staging");
      assertEquals(rmAgain.ok, false);
    } finally {
      Deno.env.delete("LEANTIME_URL");
      Deno.env.delete("LEANTIME_API_KEY");
      await clearInstanceEnv();
    }
  });
});

Deno.test("instances — readKey/readUrl are profile-scoped", async () => {
  await withTempHome(async () => {
    // default (top-level)
    await writeUrl("https://default.leantime.test");
    await writeKey("lt_default_key_0000000000000000000000000000000");

    Deno.env.set("LEANTIME_INSTANCE", "prod");
    try {
      // Profile has no files yet → null
      assertEquals(await readKey(), null);
      assertEquals(await readUrl(), null);

      // Write under the profile → read back scoped
      await writeUrl("https://prod.leantime.test");
      await writeKey(LONG_KEY);
      assertEquals(await readUrl(), "https://prod.leantime.test");
      assertEquals(await readKey(), LONG_KEY);
    } finally {
      Deno.env.delete("LEANTIME_INSTANCE");
    }

    // Default unchanged
    assertEquals(await readUrl(), "https://default.leantime.test");
  });
});

Deno.test("instances — resolveServerEnv: explicit env wins over profile", async () => {
  const hadUrl = Deno.env.get("LEANTIME_URL");
  const hadKey = Deno.env.get("LEANTIME_API_KEY");
  Deno.env.delete("LEANTIME_URL");
  Deno.env.delete("LEANTIME_API_KEY");
  try {
    await withTempHome(async () => {
      // top-level + profile, profile selected
      await writeUrl("https://default.leantime.test");
      await writeKey(LONG_KEY);
      Deno.env.set("LEANTIME_INSTANCE", "local");
      try {
        await writeUrl("http://localhost:8090");
        await writeKey("lt_local_key_0000000000000000000000000000000");

        const r = await resolveServerEnv();
        assertEquals(r.ok, true);
        assertEquals((r as { url: string }).url, "http://localhost:8090");
        assertEquals((r as { apiKey: string }).apiKey, "lt_local_key_0000000000000000000000000000000");

        // Explicit env overrides the profile
        Deno.env.set("LEANTIME_URL", "https://override.leantime.test");
        Deno.env.set("LEANTIME_API_KEY", "lt_env_override");
        try {
          const e = await resolveServerEnv();
          assertEquals((e as { url: string }).url, "https://override.leantime.test");
          assertEquals((e as { apiKey: string }).apiKey, "lt_env_override");
        } finally {
          Deno.env.delete("LEANTIME_URL");
          Deno.env.delete("LEANTIME_API_KEY");
        }
      } finally {
        Deno.env.delete("LEANTIME_INSTANCE");
      }
    });
  } finally {
    if (hadUrl !== undefined) Deno.env.set("LEANTIME_URL", hadUrl);
    if (hadKey !== undefined) Deno.env.set("LEANTIME_API_KEY", hadKey);
  }
});

Deno.test("instances — doctor reports the active profile", async () => {
  await withTempHome(async () => {
    Deno.env.delete("LEANTIME_URL");
    Deno.env.delete("LEANTIME_INSTANCE");
    await writeDefaultInstance("prod");
    await writeUrl("https://leantime.test");
    await writeKey(LONG_KEY);

    const defaultDoctor = await doctorChecks();
    const instCheck = defaultDoctor.find((c) => c.label === "default instance")!;
    assertEquals(instCheck.detail.includes('"prod"'), true);
    assertEquals(instCheck.status, "ok");

    Deno.env.set("LEANTIME_INSTANCE", "prod");
    try {
      const prodDoctor = await doctorChecks();
      const check = prodDoctor.find((c) => c.label === "default instance")!;
      assertEquals(check.detail.includes('"prod"'), true);
      assertEquals(check.detail.includes("LEANTIME_INSTANCE"), true);
    } finally {
      Deno.env.delete("LEANTIME_INSTANCE");
    }
  });
});
