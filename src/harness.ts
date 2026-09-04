/**
 * Harness setup — writes MCP server configs for the common harnesses.
 *
 * Every config is a BARE COMMAND with NO environment block: the leantmcp
 * binary resolves its credentials from the keyring (~/.config/leantime/)
 * at startup, so no secret ever lands in a harness config. The keyring is
 * created first if missing (this module prompts for it).
 */
import { dirname } from "@std/path";
import {
  hiddenPrompt,
  homeDir,
  IS_WINDOWS,
  readKey,
  readUrl,
  writeKey,
  writeUrl,
} from "./keyring.ts";

export type Harness = "claude-code" | "claude-desktop" | "cursor" | "codex";

export interface HarnessResult {
  ok: boolean;
  message: string;
}

/** Absolute path of the leantmcp executable (or deno run command for dev). */
function serverCommand(): string {
  const execPath = Deno.execPath();
  const execName = (execPath.split("/").pop() || "").split("\\").pop() || "";
  if (execName !== "deno" && execName !== "deno.exe") {
    return execPath;
  }
  // Dev mode (deno run): resolve the source path so the config still works.
  const sourcePath = new URL(import.meta.url).pathname;
  return `${execPath} run --allow-net --allow-env --allow-read --allow-write ${sourcePath}`;
}

/** Make sure the keyring exists, prompting (or reading env) for what's missing. */
export async function ensureKeyring(): Promise<HarnessResult> {
  const problems: string[] = [];
  if (!(await readUrl())) {
    const url = Deno.env.get("LEANTIME_URL")?.trim() ||
      prompt("Leantime instance URL:")?.trim() || "";
    if (url) await writeUrl(url);
    else problems.push("LEANTIME_URL");
  }
  if (!(await readKey())) {
    const key = Deno.env.get("LEANTIME_API_KEY")?.trim() ||
      await hiddenPrompt("API key (input hidden): ");
    if (key) await writeKey(key);
    else problems.push("LEANTIME_API_KEY");
  }
  if (problems.length > 0) {
    return {
      ok: false,
      message: `Missing ${problems.join(" and ")} — run 'leantmcp url set' / 'leantmcp key set' first.`,
    };
  }
  return { ok: true, message: "" };
}

// ---------------------------------------------------------------- paths

export function claudeDesktopConfigPath(): string {
  if (Deno.build.os === "darwin") {
    return `${homeDir()}/Library/Application Support/Claude/claude_desktop_config.json`;
  }
  if (Deno.build.os === "windows") {
    return `${homeDir()}/AppData/Roaming/Claude/claude_desktop_config.json`;
  }
  return `${homeDir()}/.config/Claude/claude_desktop_config.json`;
}

export function cursorConfigPath(): string {
  return `${homeDir()}/.cursor/mcp.json`;
}

export function codexConfigPath(): string {
  return `${homeDir()}/.codex/config.toml`;
}

// ---------------------------------------------------------------- writers

async function writeMcpJson(
  path: string,
  command: string,
): Promise<HarnessResult> {
  let config: Record<string, unknown> = {};
  try {
    config = JSON.parse(await Deno.readTextFile(path));
  } catch {
    // no config yet
  }
  const mcp = (config.mcpServers as Record<string, unknown>) ?? {};
  mcp.leantime = { command };
  config.mcpServers = mcp;

  await Deno.mkdir(dirname(path), { recursive: true });
  await Deno.writeTextFile(path, JSON.stringify(config, null, 2) + "\n");
  if (!IS_WINDOWS) {
    try {
      await Deno.chmod(path, 0o600);
    } catch { /* best effort */ }
  }
  return {
    ok: true,
    message:
      `Written to ${path} — bare command, no secrets (the binary reads ` +
      `~/.config/leantime/ itself).`,
  };
}

async function appendCodexToml(
  path: string,
  command: string,
): Promise<HarnessResult> {
  let existing = "";
  try {
    existing = await Deno.readTextFile(path);
  } catch {
    // no config yet
  }
  if (existing.includes("[mcp_servers.leantime]")) {
    return {
      ok: true,
      message: `${path} already has a [mcp_servers.leantime] section — left untouched. Remove it first to regenerate.`,
    };
  }
  await Deno.mkdir(dirname(path), { recursive: true });
  const block = `\n[mcp_servers.leantime]\ncommand = ${JSON.stringify(command)}\n`;
  await Deno.writeTextFile(path, existing + (existing.endsWith("\n") || existing === "" ? "" : "\n") + block);
  if (!IS_WINDOWS) {
    try {
      await Deno.chmod(path, 0o600);
    } catch { /* best effort */ }
  }
  return {
    ok: true,
    message: `Appended [mcp_servers.leantime] to ${path} — bare command, no secrets.`,
  };
}

// ---------------------------------------------------------------- entry

export async function setupHarnessCommand(
  harness: Harness,
): Promise<HarnessResult> {
  const keyring = await ensureKeyring();
  if (!keyring.ok) return keyring;

  const command = serverCommand();
  switch (harness) {
    case "claude-code": {
      // Project scope by default (.mcp.json in cwd) — the committed, shared form.
      const r = await writeMcpJson(`${Deno.cwd()}/.mcp.json`, command);
      return {
        ok: r.ok,
        message:
          `${r.message}\n` +
          `For a user-scoped server instead, run:\n` +
          `  claude mcp add leantime --scope user -- ${command}`,
      };
    }
    case "claude-desktop":
      // GUI apps get a limited PATH — the absolute path is already absolute.
      return await writeMcpJson(claudeDesktopConfigPath(), command);
    case "cursor":
      return await writeMcpJson(cursorConfigPath(), command);
    case "codex":
      return await appendCodexToml(codexConfigPath(), command);
  }
}
