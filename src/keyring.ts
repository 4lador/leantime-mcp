/**
 * Local keyring for the Leantime API key.
 *
 * Stores the key in a single dedicated file (0600, no trailing newline —
 * opencode's {file:} substitution includes file contents verbatim) so the
 * opencode config only ever holds a {file:...} pointer.
 */
import { LeantimeClient } from "./leantime-client.ts";
import type { FetchFn } from "./leantime-client.ts";

/** POSIX file modes do not exist on Windows (permissions are ACL-based, and
 * Deno.chmod is unimplemented there — it throws). */
export const IS_WINDOWS = Deno.build.os === "windows";

export function homeDir(): string {
  return Deno.env.get("HOME") ?? Deno.env.get("USERPROFILE") ?? ".";
}

export function secretDir(): string {
  return `${homeDir()}/.config/leantime`;
}

export function secretPath(): string {
  return `${secretDir()}/api-key`;
}

/** The instance URL file — configuration, not a secret (no chmod needed). */
export function instanceUrlPath(): string {
  return `${secretDir()}/instance-url`;
}

export async function writeUrl(url: string): Promise<string> {
  await Deno.mkdir(secretDir(), { recursive: true });
  const clean = url.replace(/[\r\n]+$/, "").replace(/\/+$/, "");
  await Deno.writeTextFile(instanceUrlPath(), clean);
  return instanceUrlPath();
}

export async function readUrl(): Promise<string | null> {
  try {
    const url = (await Deno.readTextFile(instanceUrlPath()))
      .replace(/[\r\n]+$/, "")
      .replace(/\/+$/, "");
    return url || null;
  } catch {
    return null;
  }
}

/** Write the key to the secret file. Returns the path written. */
export async function writeKey(key: string): Promise<string> {
  await Deno.mkdir(secretDir(), { recursive: true });
  if (!IS_WINDOWS) {
    try {
      await Deno.chmod(secretDir(), 0o700);
    } catch {
      // chmod on the existing dir can fail on some platforms — best effort
    }
  }
  // Strip any trailing newline: it would end up verbatim in the {file:} value.
  await Deno.writeTextFile(secretPath(), key.replace(/[\r\n]+$/, ""));
  if (!IS_WINDOWS) {
    await Deno.chmod(secretPath(), 0o600);
  }
  // On Windows, privacy relies on the user-profile ACLs (files under
  // %USERPROFILE% are only readable by the user by default).
  return secretPath();
}

export async function readKey(): Promise<string | null> {
  try {
    return (await Deno.readTextFile(secretPath())).replace(/[\r\n]+$/, "");
  } catch {
    return null;
  }
}

/** POSIX mode of a file as "600"-style string, or null (Windows / stat failure). */
export async function fileMode(path: string): Promise<string | null> {
  if (IS_WINDOWS) return null;
  try {
    const stat = await Deno.stat(path);
    return stat.mode !== null
      ? (stat.mode & 0o777).toString(8).padStart(3, "0")
      : null;
  } catch {
    return null;
  }
}

export async function keyFileStatus(): Promise<{
  exists: boolean;
  mode: string | null;
}> {
  try {
    await Deno.stat(secretPath());
    return { exists: true, mode: await fileMode(secretPath()) };
  } catch {
    return { exists: false, mode: null };
  }
}

/** Masked display form: lt_h13…Fc3O */
export function maskKey(key: string): string {
  if (key.length <= 10) return `${key.slice(0, 3)}…`;
  return `${key.slice(0, 6)}…${key.slice(-4)}`;
}

/** Live-validate a key against the instance. */
export async function testKey(
  url: string,
  key: string,
  fetchFn?: FetchFn,
): Promise<{ ok: boolean; message: string }> {
  try {
    const client = new LeantimeClient(url, key, fetchFn);
    const users = await client.call<unknown[]>("users.getAll");
    if (Array.isArray(users)) {
      return { ok: true, message: `valid — ${users.length} users visible` };
    }
    return { ok: false, message: "unexpected API response" };
  } catch (e) {
    return { ok: false, message: e instanceof Error ? e.message : String(e) };
  }
}

/** Resolve the instance URL: env first (override mechanism), then the keyring
 * file, then legacy configs — ignoring config values that are {file:} pointers
 * (useless as URLs here). */
export interface ResolvedUrl {
  url: string;
  source: "env" | "keyring" | "config";
}

export async function resolveUrlDetailed(): Promise<ResolvedUrl | null> {
  const fromEnv = Deno.env.get("LEANTIME_URL")?.replace(/\/+$/, "");
  if (fromEnv) return { url: fromEnv, source: "env" };

  const fromFile = await readUrl();
  if (fromFile) return { url: fromFile, source: "keyring" };

  try {
    const cfg = JSON.parse(
      await Deno.readTextFile(`${homeDir()}/.opencode/opencode.json`),
    );
    const url = cfg?.mcp?.leantime?.environment?.LEANTIME_URL;
    if (url && !String(url).startsWith("{file:")) {
      return { url: String(url).replace(/\/+$/, ""), source: "config" };
    }
  } catch {
    // no config
  }
  return null;
}

export async function resolveUrl(): Promise<string | null> {
  return (await resolveUrlDetailed())?.url ?? null;
}

/** Resolve the server credentials: environment (incl. a legacy .env loaded by
 * dotenv) first, then the keyring files. Env vars are the per-run override
 * mechanism — e.g. targeting the local docker instance for e2e tests. */
export type ServerEnv =
  | { ok: true; url: string; apiKey: string }
  | { ok: false; missing: string[] };

export async function resolveServerEnv(): Promise<ServerEnv> {
  const url = Deno.env.get("LEANTIME_URL")?.replace(/\/+$/, "") ||
    (await readUrl());
  const apiKey = Deno.env.get("LEANTIME_API_KEY") || (await readKey());

  const missing: string[] = [];
  if (!url) missing.push("LEANTIME_URL");
  if (!apiKey) missing.push("LEANTIME_API_KEY");
  if (missing.length > 0) return { ok: false, missing };

  return { ok: true, url: url!, apiKey: apiKey! };
}

/** `leantime url set [URL]` — change the instance. The URL is not a secret, so
 * a plain argument is fine (unlike the key). Configs holding {file:} pointers
 * follow automatically. */
export async function urlSetCommand(
  urlInput?: string,
  fetchFn?: FetchFn,
): Promise<CommandResult> {
  const url = urlInput?.trim() ||
    Deno.env.get("LEANTIME_URL")?.trim() ||
    prompt("Leantime instance URL:")?.trim() ||
    "";
  if (!url) {
    return { ok: false, message: "No URL provided." };
  }
  const clean = url.replace(/\/+$/, "");
  const path = await writeUrl(clean);

  // Live-verify the stored key against the new instance (best effort).
  const key = await readKey();
  let verifyNote: string;
  if (key) {
    const live = await testKey(clean, key, fetchFn);
    verifyNote = live.ok
      ? `Stored key verified against the new instance (${live.message}).`
      : `WARNING: the stored key failed against the new instance (${live.message}). If you changed instances, run: leantmcp key set`;
  } else {
    verifyNote = "No stored key yet — run: leantmcp key set";
  }

  // Legacy configs holding a plaintext URL won't follow the pointer.
  let legacyNote = "";
  try {
    const cfg = JSON.parse(
      await Deno.readTextFile(`${homeDir()}/.opencode/opencode.json`),
    );
    const cfgUrl = String(cfg?.mcp?.leantime?.environment?.LEANTIME_URL ?? "");
    if (cfgUrl && !cfgUrl.startsWith("{file:") && cfgUrl.replace(/\/+$/, "") !== clean) {
      legacyNote =
        `\nNOTE: the global opencode config still holds a plaintext URL (${cfgUrl}) — run: leantmcp setup global to switch it to a pointer.`;
    }
  } catch {
    // no config
  }

  return {
    ok: true,
    message:
      `Instance URL stored (${clean}) at ${path}.\n` +
      `${verifyNote}\n` +
      `Configs using {file:} pointers follow automatically.${legacyNote}`,
  };
}

/** `leantime url show` — resolved URL and where it comes from. */
export async function urlShowCommand(): Promise<CommandResult> {
  const r = await resolveUrlDetailed();
  if (!r) {
    return {
      ok: false,
      message: "No instance URL configured (env, keyring or config). Run: leantmcp url set <url>",
    };
  }
  return { ok: true, message: `${r.url}  (source: ${r.source})` };
}

/** Hidden (no-echo) terminal prompt. Falls back to a plain prompt when stdin
 * is not a TTY (piped input, tests, CI). */
export async function hiddenPrompt(label: string): Promise<string> {
  if (!Deno.stdin.isTerminal()) {
    return prompt(label) ?? "";
  }
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  await Deno.stdout.write(encoder.encode(label));
  Deno.stdin.setRaw(true);
  const bytes: number[] = [];
  try {
    const chunk = new Uint8Array(1);
    while (true) {
      const n = await Deno.stdin.read(chunk);
      if (n === null) break;
      const c = chunk[0];
      if (c === 13 || c === 10) break; // Enter
      if (c === 3 || c === 4) { // Ctrl+C / Ctrl+D
        await Deno.stdout.write(encoder.encode("\n"));
        Deno.exit(130);
      }
      if (c === 127 || c === 8) { // Backspace
        if (bytes.length > 0) {
          bytes.pop();
          await Deno.stdout.write(encoder.encode("\b \b"));
        }
        continue;
      }
      bytes.push(c);
      await Deno.stdout.write(encoder.encode("*"));
    }
  } finally {
    Deno.stdin.setRaw(false);
    await Deno.stdout.write(encoder.encode("\n"));
  }
  return decoder.decode(new Uint8Array(bytes)).trim();
}

export interface CommandResult {
  ok: boolean;
  message: string;
}

/** `leantime key set` — prompt (hidden) / env var / explicit input, never argv. */
export async function keySetCommand(
  keyInput?: string,
): Promise<CommandResult> {
  const key = keyInput?.trim() ||
    Deno.env.get("LEANTIME_API_KEY")?.trim() ||
    await hiddenPrompt("API key (input hidden): ");

  if (!key) {
    return { ok: false, message: "No key provided." };
  }
  const path = await writeKey(key);
  return {
    ok: true,
    message: `Key stored (${maskKey(key)}) at ${path} (0600). Configs now only need the {file:} pointer.`,
  };
}

/** `leantime key show` — masked. */
export async function keyShowCommand(): Promise<CommandResult> {
  const { exists, mode } = await keyFileStatus();
  if (!exists) {
    return {
      ok: false,
      message: `No key file at ${secretPath()}. Run: leantmcp key set`,
    };
  }
  const key = await readKey();
  const permNote = IS_WINDOWS
    ? " (ACL-managed, user profile)"
    : mode === "600"
    ? ""
    : ` — WARNING: permissions are ${mode}, run chmod 600`;
  return {
    ok: true,
    message: `${secretPath()} (${mode ?? "acl"})${permNote}\n  key: ${
      key ? maskKey(key) : "(empty)"
    }`,
  };
}

/** `leantime key test` — live validation. */
export async function keyTestCommand(fetchFn?: FetchFn): Promise<CommandResult> {
  const key = await readKey();
  if (!key) {
    return { ok: false, message: `No key file at ${secretPath()}. Run: leantmcp key set` };
  }
  const url = await resolveUrl();
  if (!url) {
    return {
      ok: false,
      message: "No instance URL configured — run: leantmcp url set <url>",
    };
  }
  const result = await testKey(url, key, fetchFn);
  return {
    ok: result.ok,
    message: `${url} → ${result.message}`,
  };
}

export interface CheckResult {
  label: string;
  status: "ok" | "warn" | "fail";
  detail: string;
}

/** `leantime key rotate` — mint a new key with the same role, verify it live
 * BEFORE replacing anything, then store it. The old key must be deleted
 * manually in the Leantime UI (no delete method exists in the API). */
export async function keyRotateCommand(
  options?: { name?: string; fetchFn?: FetchFn },
): Promise<CommandResult> {
  const currentKey = await readKey();
  if (!currentKey) {
    return {
      ok: false,
      message: `No key file at ${secretPath()}. Run: leantmcp key set`,
    };
  }
  const url = await resolveUrl();
  if (!url) {
    return {
      ok: false,
      message: "No instance URL configured — run: leantmcp url set <url>",
    };
  }

  const client = new LeantimeClient(url, currentKey, options?.fetchFn);

  // Identify the current key's entry to preserve its role.
  // Key format is lt_{user}_{password}; getAPIKeys masks usernames to 5 chars.
  const userSegment = currentKey.replace(/^lt_/, "").split("_")[0];
  const keys = await client.call<
    { id: unknown; username?: string; role?: string }[]
  >("Api.getAPIKeys", {});
  const entry = (keys ?? []).find((k) =>
    userSegment.startsWith(String(k.username ?? "").slice(0, 5))
  );
  if (!entry) {
    return {
      ok: false,
      message:
        "Could not identify the current key in the instance's API key list — rotation aborted.",
    };
  }

  const name = options?.name ?? `MCP-rotated-${new Date().toISOString().slice(0, 10)}`;
  // source: "api" is REQUIRED — the service alone does not set it, and without
  // it the created key is rejected at authentication (401). Only the web UI
  // controller sets it; we must pass it explicitly.
  // Project relations must be preserved too — a fresh key is assigned to NO
  // project and would see nothing.
  const created = await client.call<
    { id?: unknown; user?: string; passwordClean?: string; password?: string } | false
  >(
    "Api.createAPIKey",
    { values: { firstname: name, role: entry.role, source: "api" } },
  );
  if (!created || !created.user) {
    return {
      ok: false,
      message: "Key creation failed on the instance — rotation aborted.",
    };
  }

  // Copy the old key's project assignments onto the new one (best effort).
  let relationsNote = "";
  try {
    const assigned = await client.call<{ id: unknown }[]>(
      "Projects.getProjectsAssignedToUser",
      { userId: String(entry.id) },
    );
    const projectIds = (assigned ?? []).map((p) => String(p.id));
    if (projectIds.length > 0 && created.id !== undefined) {
      const rel = await client.call<boolean>(
        "Projects.editUserProjectRelations",
        { id: String(created.id), projects: projectIds },
      );
      if (rel !== true) {
        relationsNote =
          "\nWARNING: could not copy project assignments to the new key — assign them in the Leantime UI.";
      }
    }
  } catch {
    relationsNote =
      "\nWARNING: could not copy project assignments to the new key — assign them in the Leantime UI.";
  }

  // The secret only ever exists in this process's memory.
  const newKey = `lt_${created.user}_${created.passwordClean ?? created.password}`;

  // Verify the new key live BEFORE touching the stored one.
  const live = await testKey(url, newKey, options?.fetchFn);
  if (!live.ok) {
    return {
      ok: false,
      message:
        `New key verification failed (${live.message}) — rotation aborted, ` +
        "the previous key is untouched.",
    };
  }

  const oldMasked = maskKey(currentKey);
  await writeKey(newKey);
  return {
    ok: true,
    message:
      `Key rotated: ${maskKey(newKey)} (role ${entry.role}, name "${name}") stored at ${secretPath()}.` +
      relationsNote +
      `\nUpdate any other consumers that embed the old key (e.g. CI secrets).\n` +
      `Now delete the old key ${oldMasked} in the Leantime UI (Company Settings → API Keys).`,
  };
}

/** `leantmcp doctor` — collect all health checks without exiting. */
export async function doctorChecks(fetchFn?: FetchFn): Promise<CheckResult[]> {
  const results: CheckResult[] = [];

  // Secret file
  const { exists, mode } = await keyFileStatus();
  if (!exists) {
    results.push({
      label: "key file",
      status: "fail",
      detail: `${secretPath()} missing — run: leantmcp key set`,
    });
  } else {
    const permOk = IS_WINDOWS || mode === "600";
    results.push({
      label: "key file",
      status: permOk ? "ok" : "warn",
      detail: IS_WINDOWS
        ? `${secretPath()} (ACL-managed, user profile)`
        : mode === "600"
        ? `${secretPath()} (0600)`
        : `${secretPath()} has mode ${mode} — run: chmod 600 ${secretPath()}`,
    });
  }

  // Instance URL (with source)
  const resolved = await resolveUrlDetailed();
  results.push({
    label: "instance URL",
    status: resolved ? "ok" : "warn",
    detail: resolved
      ? `${resolved.url} (source: ${resolved.source})`
      : "not found — run: leantmcp url set <url>",
  });

  // Live key validation
  const key = await readKey();
  if (key && resolved) {
    const live = await testKey(resolved.url, key, fetchFn);
    results.push({
      label: "key validation",
      status: live.ok ? "ok" : "fail",
      detail: live.message,
    });
  }

  // Global opencode config — expect BOTH the key and URL to be pointers
  const globalConfig = `${homeDir()}/.opencode/opencode.json`;
  try {
    const cfg = JSON.parse(await Deno.readTextFile(globalConfig));
    const env = cfg?.mcp?.leantime?.environment ?? {};
    const storedKey = String(env.LEANTIME_API_KEY ?? "");
    const storedUrl = String(env.LEANTIME_URL ?? "");
    const keyPointer = storedKey.startsWith("{file:");
    const urlPointer = storedUrl.startsWith("{file:");
    const cfgMode = await fileMode(globalConfig);
    const permOk = IS_WINDOWS || cfgMode === "600";
    const allPointers = keyPointer && urlPointer;
    results.push({
      label: "global opencode config",
      status: allPointers && permOk ? "ok" : "warn",
      detail:
        `${globalConfig} (mode ${cfgMode ?? "acl"}) — ` +
        `LEANTIME_API_KEY is ${keyPointer ? "a pointer" : storedKey ? "plaintext" : "missing"}, ` +
        `LEANTIME_URL is ${urlPointer ? "a pointer" : storedUrl ? "plaintext" : "missing"}` +
        `${allPointers ? "" : " — run: leantmcp setup global"}`,
    });
  } catch {
    results.push({
      label: "global opencode config",
      status: "warn",
      detail: `${globalConfig} not found — run: leantmcp setup global`,
    });
  }

  // .env in the current directory holding a key copy (stale or duplicate)
  try {
    const envContent = await Deno.readTextFile("./.env");
    const match = envContent.match(/^LEANTIME_API_KEY=(.+)$/m);
    if (match) {
      const envKey = match[1].trim();
      const stored = await readKey();
      results.push({
        label: ".env key copy",
        status: "warn",
        detail: stored && envKey === stored
          ? "./.env embeds a duplicate of the stored key — remove the line, the keyring is authoritative"
          : "./.env embeds a DIFFERENT key than the keyring — likely stale (401s ahead), remove the line",
      });
    }
  } catch {
    // no .env — the normal state
  }

  return results;
}
