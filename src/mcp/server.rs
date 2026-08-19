//! The unix-socket MCP server and the stdio bridge that fronts it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use iced::futures::Stream;
use iced::futures::channel::{mpsc, oneshot};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use super::{
    Job, Request, Response, initialize_result, resource_definitions, tool_error, tool_result,
    tools::tools_list_json,
};

/// The environment variable a client can set to point at a different socket.
pub const SOCKET_ENV: &str = "LIMEDAW_MCP_SOCKET";

/// How long a tool call waits for the application to answer.
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The receiving half of the job channel, parked here so the iced subscription
/// can pick it up without threading it through the application state.
static JOB_RX: Mutex<Option<mpsc::UnboundedReceiver<Job>>> = Mutex::new(None);

/// Take the job stream. Returns an empty stream if it has already been taken.
pub fn job_stream() -> impl Stream<Item = Job> {
    use iced::futures::StreamExt;
    let receiver = JOB_RX.lock().ok().and_then(|mut slot| slot.take());
    match receiver {
        Some(rx) => rx.left_stream(),
        None => iced::futures::stream::pending().right_stream(),
    }
}

/// A running server. Dropping it stops accepting and removes the socket file.
pub struct ServerHandle {
    pub socket_path: PathBuf,
    shutdown: Option<oneshot::Sender<()>>,
    _runtime: tokio::runtime::Runtime,
}

impl ServerHandle {
    /// Bind the socket and start serving. The job channel is installed for
    /// [`job_stream`] to pick up.
    pub fn start(socket_path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        clear_stale_socket(&socket_path)?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("limedaw-mcp")
            .build()?;

        let (job_tx, job_rx) = mpsc::unbounded::<Job>();
        if let Ok(mut slot) = JOB_RX.lock() {
            *slot = Some(job_rx);
        }

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let listener = {
            let _guard = runtime.enter();
            UnixListener::bind(&socket_path)?
        };

        // A socket in the user's own data directory; make that explicit.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600));
        }

        let path_for_log = socket_path.clone();
        runtime.spawn(async move {
            log::info!("MCP server listening on {}", path_for_log.display());
            let mut shutdown = shutdown_rx;
            loop {
                tokio::select! {
                    _ = &mut shutdown => break,
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _)) => {
                            let tx = job_tx.clone();
                            tokio::spawn(async move {
                                if let Err(err) = serve_connection(stream, tx).await {
                                    log::debug!("MCP connection closed: {err}");
                                }
                            });
                        }
                        Err(err) => {
                            log::error!("MCP accept failed: {err}");
                            break;
                        }
                    },
                }
            }
        });

        Ok(Self {
            socket_path,
            shutdown: Some(shutdown_tx),
            _runtime: runtime,
        })
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Remove a socket file left behind by a previous run, but never one that a
/// live server is still listening on.
fn clear_stale_socket(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(_) => Err(anyhow::anyhow!(
            "another LimeDAW is already serving MCP on {}",
            path.display()
        )),
        Err(_) => {
            std::fs::remove_file(path)?;
            Ok(())
        }
    }
}

async fn serve_connection(
    stream: UnixStream,
    jobs: mpsc::UnboundedSender<Job>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Recover the id before anything else: a client whose request comes
        // back with a null id would wait for a reply that never matches.
        let envelope = serde_json::from_str::<Value>(trimmed);
        let id = envelope
            .as_ref()
            .ok()
            .and_then(|v| v.get("id").cloned())
            .filter(|v| !v.is_null());

        let response = match envelope {
            Err(err) => Some(Response::err(
                id.unwrap_or(Value::Null),
                -32700,
                format!("parse error: {err}"),
            )),
            Ok(value) => match serde_json::from_value::<Request>(value) {
                Ok(request) => dispatch(request, &jobs).await,
                Err(err) => {
                    id.map(|id| Response::err(id, -32600, format!("invalid request: {err}")))
                }
            },
        };
        if let Some(response) = response {
            let mut encoded = serde_json::to_vec(&response)?;
            encoded.push(b'\n');
            writer.write_all(&encoded).await?;
            writer.flush().await?;
        }
    }
    Ok(())
}

async fn dispatch(request: Request, jobs: &mpsc::UnboundedSender<Job>) -> Option<Response> {
    let id = request.id.clone();
    // Notifications carry no id and get no reply.
    let is_notification = id.is_none();
    let id = id.unwrap_or(Value::Null);

    let result: Result<Value, (i32, String)> = match request.method.as_str() {
        "initialize" => Ok(initialize_result()),
        "notifications/initialized" | "notifications/cancelled" => Ok(json!({})),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list_json()),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        "resources/list" => Ok(resource_definitions()),
        "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
        "logging/setLevel" => Ok(json!({})),
        "resources/read" => {
            let uri = request
                .params
                .get("uri")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let tool = match uri.as_str() {
                "limedaw://project" => "get_project",
                "limedaw://state" => "get_state",
                "limedaw://tools" => "",
                _ => {
                    return finish(
                        is_notification,
                        Response::err(id, -32602, format!("unknown resource: {uri}")),
                    );
                }
            };
            let payload = if tool.is_empty() {
                Ok(tools_list_json())
            } else {
                run_tool(jobs, tool, json!({})).await
            };
            match payload {
                Ok(value) => Ok(json!({
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": serde_json::to_string_pretty(&value).unwrap_or_default(),
                    }]
                })),
                Err(message) => Err((-32603, message)),
            }
        }
        "tools/call" => {
            let name = request
                .params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if name.is_empty() {
                Err((-32602, "tools/call requires a tool name".into()))
            } else {
                // Tool failures are reported inside the result, not as
                // protocol errors, which is what MCP clients expect.
                match run_tool(jobs, &name, arguments).await {
                    Ok(value) => Ok(tool_result(value)),
                    Err(message) => Ok(tool_error(&message)),
                }
            }
        }
        other => Err((-32601, format!("unknown method: {other}"))),
    };

    let response = match result {
        Ok(value) => Response::ok(id, value),
        Err((code, message)) => Response::err(id, code, message),
    };
    finish(is_notification, response)
}

fn finish(is_notification: bool, response: Response) -> Option<Response> {
    if is_notification {
        None
    } else {
        Some(response)
    }
}

/// Hand a tool call to the application and wait for its answer.
async fn run_tool(
    jobs: &mpsc::UnboundedSender<Job>,
    tool: &str,
    arguments: Value,
) -> Result<Value, String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    let job = Job {
        tool: tool.to_string(),
        arguments,
        reply: reply_tx,
    };
    jobs.unbounded_send(job)
        .map_err(|_| "LimeDAW is not accepting commands".to_string())?;

    match tokio::time::timeout(CALL_TIMEOUT, reply_rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("LimeDAW dropped the request".into()),
        Err(_) => Err("timed out waiting for LimeDAW".into()),
    }
}

/// Resolve the socket path: the environment wins, then the default location.
pub fn socket_path() -> PathBuf {
    std::env::var_os(SOCKET_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(crate::paths::mcp_socket)
}

/// Bridge stdin/stdout onto the unix socket, so MCP clients that only know how
/// to spawn a process can talk to the running app.
pub fn stdio_bridge() -> anyhow::Result<()> {
    let path = socket_path();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let stream = match UnixStream::connect(&path).await {
            Ok(stream) => stream,
            Err(err) => {
                // Answering `initialize` even when the app is closed gives the
                // client a readable error instead of a silent failure.
                return offline_stub(&path, err).await;
            }
        };

        let (socket_reader, mut socket_writer) = stream.into_split();
        let mut stdin = BufReader::new(tokio::io::stdin()).lines();
        let mut socket_lines = BufReader::new(socket_reader).lines();
        let mut stdout = tokio::io::stdout();

        loop {
            tokio::select! {
                line = stdin.next_line() => match line? {
                    Some(line) => {
                        socket_writer.write_all(line.as_bytes()).await?;
                        socket_writer.write_all(b"\n").await?;
                        socket_writer.flush().await?;
                    }
                    None => break,
                },
                line = socket_lines.next_line() => match line? {
                    Some(line) => {
                        stdout.write_all(line.as_bytes()).await?;
                        stdout.write_all(b"\n").await?;
                        stdout.flush().await?;
                    }
                    None => break,
                },
            }
        }
        Ok::<(), anyhow::Error>(())
    })?;

    // A blocking read on stdin cannot be cancelled, so the runtime would keep
    // the process alive after the socket closed. Leave deliberately.
    std::process::exit(0);
}

/// A minimal server that only reports that LimeDAW is not running.
async fn offline_stub(path: &Path, err: std::io::Error) -> anyhow::Result<()> {
    let message = format!(
        "LimeDAW is not running, or its MCP socket is unavailable at {} ({err}). \
         Start LimeDAW and make sure the MCP server is enabled in Settings.",
        path.display()
    );
    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = stdin.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Request>(trimmed) else {
            continue;
        };
        let Some(id) = request.id.clone() else {
            continue;
        };
        let response = match request.method.as_str() {
            "initialize" => Response::ok(id, initialize_result()),
            "tools/list" => Response::ok(id, tools_list_json()),
            "ping" => Response::ok(id, json!({})),
            "tools/call" => Response::ok(id, tool_error(&message)),
            _ => Response::err(id, -32000, message.clone()),
        };
        let mut encoded = serde_json::to_vec(&response)?;
        encoded.push(b'\n');
        stdout.write_all(&encoded).await?;
        stdout.flush().await?;
    }
    Ok(())
}
