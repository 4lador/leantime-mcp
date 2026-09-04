/**
 * MCP handshake smoke test: spawns the compiled leantmcp binary over stdio,
 * performs the initialize handshake and asserts the exposed tool list matches
 * the in-repo registry exactly. Needs no Leantime instance (handshake only).
 *
 * Usage: LEANTMCP_BIN=./leantmcp deno run --allow-run --allow-env scripts/smoke.ts
 */
import { registerAllTools } from "../src/tools/mod.ts";

const bin = Deno.env.get("LEANTMCP_BIN");
if (!bin) {
  console.error("LEANTMCP_BIN is required (path to the leantmcp binary)");
  Deno.exit(1);
}

// Expected tool set = what the in-repo registry defines (single source of truth).
const expected = new Set<string>();
registerAllTools(
  {
    tool: (name: string) => {
      expected.add(name);
    },
  } as never,
  null as never,
);

const proc = new Deno.Command(bin, {
  stdin: "piped",
  stdout: "piped",
  env: {
    LEANTIME_URL: "http://smoke.invalid",
    LEANTIME_API_KEY: "lt_smoke",
  },
}).spawn();

const writer = proc.stdin.getWriter();
const reader = proc.stdout.getReader();
const decoder = new TextDecoder();
let buffer = "";

function send(obj: unknown) {
  writer.write(new TextEncoder().encode(JSON.stringify(obj) + "\n"));
}

function readMsg(): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    const tryRead = async () => {
      const { value, done } = await reader.read();
      if (done) {
        reject(new Error("server closed stdout"));
        return;
      }
      buffer += decoder.decode(value);
      const nl = buffer.indexOf("\n");
      if (nl === -1) {
        await tryRead();
        return;
      }
      const line = buffer.slice(0, nl);
      buffer = buffer.slice(nl + 1);
      resolve(JSON.parse(line));
    };
    tryRead().catch(reject);
  });
}

let failures = 0;
const check = (cond: boolean, label: string) => {
  console.log((cond ? "[OK ] " : "[FAIL] ") + label);
  if (!cond) failures++;
};

try {
  send({
    jsonrpc: "2.0", id: 1, method: "initialize",
    params: {
      protocolVersion: "2024-11-05", capabilities: {},
      clientInfo: { name: "smoke", version: "0" },
    },
  });
  const init = await readMsg();
  const serverInfo = (init.result as { serverInfo?: { name?: string } }).serverInfo;
  check(serverInfo?.name === "leantime-mcp", `initialize handshake (server: ${serverInfo?.name})`);

  send({ jsonrpc: "2.0", method: "notifications/initialized" });

  send({ jsonrpc: "2.0", id: 2, method: "tools/list" });
  const tools = await readMsg();
  const exposed = new Set(
    ((tools.result as { tools: { name: string }[] }).tools).map((t) => t.name),
  );
  check(
    exposed.size === expected.size && [...expected].every((n) => exposed.has(n)),
    `tools/list matches registry (${exposed.size}/${expected.size} tools)`,
  );
} finally {
  try {
    writer.releaseLock();
    await proc.stdin.close();
  } catch {
    // already closed
  }
  try {
    proc.kill();
  } catch {
    // already exited
  }
}

if (failures > 0) Deno.exit(1);
console.log("SMOKE PASSED");
