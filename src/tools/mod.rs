//! The 42 MCP tools, split by domain. [`tools::create_registry`] returns them
//! all; handlers close over a shared [`tools::ClientRef`].

mod backup;
mod bulk;
mod comments;
mod key_rotate;
mod milestones;
mod project_context;
mod projects;
mod shared;
mod sprints;
mod tickets;
mod timesheets;
mod users;

pub use key_rotate::key_rotate;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::client::LeantimeClient;

/// Shared handle to the (serialised) API client.
pub type ClientRef = Arc<Mutex<LeantimeClient>>;

/// A tool handler: boxed async closure taking (args, client) → MCP result.
pub type Handler =
    Box<dyn Fn(Value, ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync>;

/// One registered MCP tool.
pub struct Tool {
    /// Tool name (`leantime_<domain>_<action>`).
    pub name: &'static str,
    /// Human/agent-facing description.
    pub description: String,
    /// JSON Schema of the input arguments.
    pub schema: Value,
    /// MCP tool annotations (behavioral hints for clients).
    pub annotations: ToolAnnotations,
    /// The handler closure.
    pub handler: Handler,
}

/// MCP ToolAnnotations — behavioral hints for clients (per the MCP spec,
/// these are advisory, not guarantees). Clients use them for UI grouping,
/// approval workflows and caching decisions.
#[derive(Debug, Clone)]
pub struct ToolAnnotations {
    /// If true, the tool does not modify its environment.
    pub read_only: bool,
    /// If true, the tool may perform destructive updates (data loss).
    pub destructive: bool,
    /// If true, calling the tool repeatedly with the same arguments
    /// has the same effect as calling it once.
    pub idempotent: bool,
    /// If true, the tool interacts with an "open world" of external entities.
    /// All our tools call a remote Leantime API.
    pub open_world: bool,
}

impl ToolAnnotations {
    /// Serialize to the MCP JSON wire format (false values omitted — the
    /// spec treats absent as false, keeping the tools/list payload compact).
    pub fn to_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        if self.read_only {
            map.insert("readOnlyHint".into(), Value::Bool(true));
        }
        if self.destructive {
            map.insert("destructiveHint".into(), Value::Bool(true));
        }
        if self.idempotent {
            map.insert("idempotentHint".into(), Value::Bool(true));
        }
        map.insert("openWorldHint".into(), Value::Bool(self.open_world));
        Value::Object(map)
    }

    /// Read-only tool: lists data, never modifies. Idempotent (same query
    /// → same result, assuming no concurrent writes).
    pub fn readonly() -> Self {
        Self {
            read_only: true,
            destructive: false,
            idempotent: true,
            open_world: true,
        }
    }

    /// Destructive tool: deletes data. Idempotent (deleting twice = same
    /// end state — the second call errors but the data is still gone).
    pub fn destructive() -> Self {
        Self {
            read_only: false,
            destructive: true,
            idempotent: true,
            open_world: true,
        }
    }

    /// Write tool: creates or updates data. NOT idempotent (creating twice
    /// = two items; updating may trigger side effects).
    pub fn write() -> Self {
        Self {
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: true,
        }
    }
}

/// Wrap an error message as an MCP tool error result.
pub fn error_result(message: &str) -> Value {
    serde_json::json!({
        "content": [{ "type": "text", "text": format!("Error: {}", message) }],
        "isError": true
    })
}

/// Wrap a JSON payload as a successful MCP tool result.
pub fn ok_result(data: &Value) -> Value {
    serde_json::json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(data).unwrap_or_default() }]
    })
}

/// Generic RPC passthrough handler factory.
fn rpc(method: &'static str, map: fn(&Value) -> Value) -> Handler {
    Box::new(
        move |args: Value, client: ClientRef| -> Pin<Box<dyn Future<Output = Value> + Send>> {
            Box::pin(async move {
                let mut c = client.lock().await;
                let params = map(&args);
                match c.call(method, params).await {
                    Ok(r) => ok_result(&r),
                    Err(e) => error_result(&e.to_string()),
                }
            })
        },
    )
}

/// All 42 tools, in stable registry order (projects, tickets, milestones,
/// sprints, users, comments, timesheets, bulk, backup, project_context).
pub fn create_registry() -> Vec<Tool> {
    let mut tools = Vec::with_capacity(42);
    tools.extend(projects::tools());
    tools.extend(tickets::tools());
    tools.extend(milestones::tools());
    tools.extend(sprints::tools());
    tools.extend(users::tools());
    tools.extend(comments::tools());
    tools.extend(timesheets::tools());
    tools.extend(bulk::tools());
    tools.extend(backup::tools());
    tools.extend(project_context::tools());
    tools
}
