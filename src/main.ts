import "@std/dotenv/load";
import { McpServer } from "@mcp/server";
import { StdioServerTransport } from "@mcp/stdio";
import { dirname } from "@std/path";
import { LeantimeClient } from "./leantime-client.ts";
import { registerAllTools } from "./tools/mod.ts";
import {
  doctorChecks,
  keyRotateCommand,
  keySetCommand,
  keyShowCommand,
  keyTestCommand,
  readKey,
  secretPath,
} from "./keyring.ts";
import { IS_WINDOWS } from "./keyring.ts";

const VERSION = "1.4.1";

export function showHelp() {
  console.log(`leantmcp v${VERSION} — Leantime MCP Server

USAGE
  leantmcp [command]

COMMANDS
  serve              Start the MCP server (default when no command given)
  setup global       Configure leantime in ~/.opencode/opencode.json
  setup project      Configure leantime in ./opencode.json
  key set            Store the API key in ~/.config/leantime/api-key (0600, hidden prompt)
  key show           Show the stored key, masked
  key test           Validate the stored key against the instance
  key rotate         Mint a new key (same role), verify it live, replace the stored one
  doctor             Health check: key file, config, live key validation
  --help, -h         Show this help
  --version, -v      Show version

SETUP
  leantmcp key set
    Stores the key in a single 0600 file. Set LEANTIME_API_KEY to skip the prompt.

  leantmcp setup global
    Prompts for Leantime URL and API key.
    Writes config to ~/.opencode/opencode.json (global).
    When the key is already stored (key set), the config only gets a
    {file:...} pointer — no plaintext secret in the config.

  leantmcp setup project
    Same, but writes ./opencode.json (current project).

  Environment variables LEANTIME_URL and LEANTIME_API_KEY can be
  set to skip prompts.

SERVE
  Reads LEANTIME_URL and LEANTIME_API_KEY from environment or .env file.
  Communicates over stdin/stdout (stdio transport).`);
}

export function showVersion() {
  console.log(`leantmcp v${VERSION}`);
}

export function getMcpCommand(): string[] {
  const execPath = Deno.execPath();
  const execName = (execPath.split("/").pop() || "").split("\\").pop() || "";
  if (execName !== "deno" && execName !== "deno.exe") {
    return [execPath];
  }
  const sourcePath = new URL(import.meta.url).pathname;
  return [execPath, "run", "--allow-net", "--allow-env", "--allow-read", "--allow-write", sourcePath];
}

export async function readJsonFile(path: string): Promise<Record<string, unknown>> {
  try {
    const content = await Deno.readTextFile(path);
    return JSON.parse(content);
  } catch {
    return {};
  }
}

export async function writeJsonFile(path: string, data: unknown): Promise<void> {
  const dir = dirname(path);
  if (dir) {
    await Deno.mkdir(dir, { recursive: true });
  }
  await Deno.writeTextFile(path, JSON.stringify(data, null, 2) + "\n");
}

export function getSetupPath(global: boolean): string {
  if (global) {
    // HOME is not defined on Windows — fall back to USERPROFILE.
    const home = Deno.env.get("HOME") ?? Deno.env.get("USERPROFILE");
    return `${home}/.opencode/opencode.json`;
  }
  return `${Deno.cwd()}/opencode.json`;
}

export function buildMergedConfig(
  existing: Record<string, unknown>,
  mcpCommand: string[],
  leantimeUrl: string,
  apiKey: string,
): Record<string, unknown> {
  const mcp = (existing.mcp as Record<string, unknown>) || {};
  return {
    ...existing,
    mcp: {
      ...mcp,
      leantime: {
        type: "local",
        command: mcpCommand,
        environment: {
          LEANTIME_URL: leantimeUrl,
          LEANTIME_API_KEY: apiKey,
        },
      },
    },
  };
}

export class SetupError extends Error {
  constructor(message: string) {
    super(message);
  }
}

export async function setupConfig(
  global: boolean,
  options?: { url?: string; apiKey?: string },
): Promise<string> {
  let leantimeUrl = options?.url ?? Deno.env.get("LEANTIME_URL") ?? "";
  let apiKey = options?.apiKey ?? Deno.env.get("LEANTIME_API_KEY") ?? "";

  if (!leantimeUrl) {
    leantimeUrl = prompt("Leantime URL:") ?? "";
    if (!leantimeUrl) {
      throw new SetupError("Leantime URL is required.");
    }
  }

  if (!apiKey) {
    apiKey = prompt("API Key:") ?? "";
    if (!apiKey) {
      throw new SetupError("API Key is required.");
    }
  }

  leantimeUrl = leantimeUrl.replace(/\/+$/, "");

  // When the key already lives in the secret file, store only a {file:...}
  // pointer in the config — opencode substitutes file contents natively.
  const storedKey = await readKey();
  const apiKeyValue = storedKey === apiKey
    ? `{file:${secretPath()}}`
    : apiKey;

  const targetPath = getSetupPath(global);
  const existing = await readJsonFile(targetPath);
  const mcpCommand = getMcpCommand();
  const merged = buildMergedConfig(existing, mcpCommand, leantimeUrl, apiKeyValue);

  await writeJsonFile(targetPath, merged);
  if (!IS_WINDOWS) {
    try {
      await Deno.chmod(targetPath, 0o600);
    } catch {
      // best effort
    }
  }
  if (apiKeyValue === apiKey) {
    console.error(
      `! API key stored in plaintext in ${targetPath}. Run 'leantmcp key set' ` +
        `first to keep the secret in a single 0600 file and a {file:} pointer here.`,
    );
  }
  return targetPath;
}

async function serve(): Promise<void> {
  const leantimeUrl = Deno.env.get("LEANTIME_URL");
  const leantimeApiKey = Deno.env.get("LEANTIME_API_KEY");

  if (!leantimeUrl || !leantimeApiKey) {
    console.error(
      "Missing LEANTIME_URL or LEANTIME_API_KEY environment variables.",
    );
    console.error("Create a .env file based on .env.example or run: leantmcp setup");
    Deno.exit(1);
  }

  const client = new LeantimeClient(leantimeUrl, leantimeApiKey);

  const server = new McpServer({
    name: "leantime-mcp",
    version: VERSION,
  });

  registerAllTools(server, client);

  const transport = new StdioServerTransport();
  await server.connect(transport);
}

// CLI dispatch — only when run as the entrypoint (deno run / compiled binary),
// never when imported (e.g. by tests).
if (import.meta.main) {
  const args = Deno.args;

  if (args.length === 0 || args[0] === "serve") {
    await serve();
  } else if (args[0] === "--help" || args[0] === "-h") {
    showHelp();
  } else if (args[0] === "--version" || args[0] === "-v") {
    showVersion();
  } else if (args[0] === "key") {
    const sub = args[1];
    if (sub === "set") {
      const r = await keySetCommand();
      console.log(r.ok ? `✓ ${r.message}` : `✗ ${r.message}`);
      if (!r.ok) Deno.exit(1);
    } else if (sub === "show") {
      const r = await keyShowCommand();
      console.log(r.message);
      if (!r.ok) Deno.exit(1);
    } else if (sub === "test") {
      const r = await keyTestCommand();
      console.log(r.ok ? `✓ ${r.message}` : `✗ ${r.message}`);
      if (!r.ok) Deno.exit(1);
    } else if (sub === "rotate") {
      const nameFlag = args.indexOf("--name");
      const name = nameFlag !== -1 ? args[nameFlag + 1] : undefined;
      const r = await keyRotateCommand({ name });
      console.log(r.ok ? `✓ ${r.message}` : `✗ ${r.message}`);
      if (!r.ok) Deno.exit(1);
    } else {
      console.error("Usage: leantmcp key set|show|test|rotate");
      Deno.exit(1);
    }
  } else if (args[0] === "doctor") {
    const results = await doctorChecks();
    let failed = false;
    for (const c of results) {
      const icon = c.status === "ok" ? "✓" : c.status === "warn" ? "!" : "✗";
      if (c.status === "fail") failed = true;
      console.log(`${icon} ${c.label}: ${c.detail}`);
    }
    if (failed) Deno.exit(1);
  } else if (args[0] === "setup") {
    const sub = args[1];
    if (sub === "global") {
      const target = await setupConfig(true);
      console.log(`✓ Written to ${target}`);
    } else if (sub === "project") {
      const target = await setupConfig(false);
      console.log(`✓ Written to ${target}`);
    } else {
      console.error("Usage: leantmcp setup global|project");
      Deno.exit(1);
    }
  } else {
    console.error(`Unknown command: ${args[0]}`);
    showHelp();
    Deno.exit(1);
  }
}
