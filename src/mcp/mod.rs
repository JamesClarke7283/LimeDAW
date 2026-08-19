//! An MCP server that exposes the whole DAW over a unix socket.
//!
//! The transport is newline-delimited JSON-RPC 2.0 — the same framing MCP uses
//! over stdio, just carried on a socket so the running app can be driven by any
//! number of clients while it stays open. `limedaw --mcp-stdio` bridges a
//! client that only knows how to spawn a process onto that socket.

pub mod tools;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use tools::{ToolDef, tool_definitions};

/// The MCP revision this server speaks.
pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const SERVER_NAME: &str = "limedaw";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A tool invocation waiting for the application to run it.
#[derive(Debug)]
pub struct Job {
    pub tool: String,
    pub arguments: Value,
    pub reply: JobReply,
}

/// Where a job's result goes. This uses the futures channel iced already
/// depends on, so the receiving end plugs straight into a subscription.
pub type JobReply = iced::futures::channel::oneshot::Sender<Result<Value, String>>;

impl Job {
    pub fn respond(self, result: Result<Value, String>) {
        let _ = self.reply.send(result);
    }
}

/// A JSON-RPC request as it arrives on the wire.
#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl Response {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// The `initialize` result.
pub fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": { "subscribe": false, "listChanged": false },
            "prompts": { "listChanged": false },
            "logging": {}
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "title": "LimeDAW",
            "version": SERVER_VERSION
        },
        "instructions": INSTRUCTIONS
    })
}

pub const INSTRUCTIONS: &str = "\
LimeDAW is a pattern-based DAW. Music is built from channels (instruments), \
patterns (notes for those channels) and a playlist (patterns arranged over time).

A typical session: call `get_state` to see what exists, `add_channel` to create \
instruments, `add_notes` to write a pattern, `add_clip` to place that pattern in \
the playlist, then `play`. Positions are in beats (a quarter note is 1.0); keys \
are MIDI numbers where 60 is middle C. Use `set_play_mode` with \"pattern\" to \
loop the pattern you are editing, or \"song\" to hear the arrangement.";

/// The resources the server publishes.
pub fn resource_definitions() -> Value {
    json!({
        "resources": [
            {
                "uri": "limedaw://project",
                "name": "Current project",
                "description": "The full project as JSON: channels, patterns, playlist and mixer.",
                "mimeType": "application/json"
            },
            {
                "uri": "limedaw://state",
                "name": "Transport and session state",
                "description": "Playback position, tempo, selection and engine status.",
                "mimeType": "application/json"
            },
            {
                "uri": "limedaw://tools",
                "name": "Tool reference",
                "description": "Every tool this server exposes, with its schema.",
                "mimeType": "application/json"
            }
        ]
    })
}

/// Wrap a tool result in MCP's content envelope.
pub fn tool_result(value: Value) -> Value {
    let text = match &value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    };
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value,
        "isError": false
    })
}

pub fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    })
}

/// The client configuration users copy into their MCP client.
///
/// Two shapes are offered: the built-in bridge (no extra software) and a
/// `socat` bridge for anyone who would rather not point at the binary.
pub fn client_config_json(exe: &str, socket: &str) -> String {
    let value = json!({
        "mcpServers": {
            "limedaw": {
                "command": exe,
                "args": ["--mcp-stdio"],
                "env": { "LIMEDAW_MCP_SOCKET": socket }
            }
        }
    });
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

/// The same thing, but bridged with `socat` instead of the LimeDAW binary.
pub fn client_config_socat(socket: &str) -> String {
    let value = json!({
        "mcpServers": {
            "limedaw": {
                "command": "socat",
                "args": ["STDIO", format!("UNIX-CONNECT:{socket}")]
            }
        }
    });
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

/// A one-line command for adding the server to Claude Code.
pub fn cli_command(exe: &str, socket: &str) -> String {
    format!("claude mcp add limedaw --env LIMEDAW_MCP_SOCKET={socket} -- {exe} --mcp-stdio")
}

#[cfg(not(target_arch = "wasm32"))]
pub mod server;
#[cfg(not(target_arch = "wasm32"))]
pub use server::{ServerHandle, stdio_bridge};

/// A stand-in for the browser build, which has no unix sockets. It is never
/// constructed; it exists so the application can hold an `Option<ServerHandle>`
/// without cfg noise spreading through the interface.
#[cfg(target_arch = "wasm32")]
pub enum ServerHandle {}
