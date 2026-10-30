//! Platform-specific local connections. Both transports use the same MCP framing.

#[cfg(unix)]
mod platform {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use tokio::net::{UnixListener, UnixStream};

    pub struct Listener(UnixListener);

    impl Listener {
        pub fn bind(path: &Path) -> anyhow::Result<Self> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            super::super::clear_stale_socket(path)?;
            let listener = UnixListener::bind(path)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            Ok(Self(listener))
        }

        pub async fn accept(&mut self) -> std::io::Result<UnixStream> {
            self.0.accept().await.map(|(stream, _)| stream)
        }
    }

    pub async fn connect(path: &Path) -> std::io::Result<UnixStream> {
        UnixStream::connect(path).await
    }
}

#[cfg(windows)]
mod platform {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };

    pub struct Listener {
        server: NamedPipeServer,
        path: PathBuf,
    }

    impl Listener {
        pub fn bind(path: &Path) -> anyhow::Result<Self> {
            let server = ServerOptions::new()
                .first_pipe_instance(true)
                .reject_remote_clients(true)
                .create(path)?;
            Ok(Self {
                server,
                path: path.to_path_buf(),
            })
        }

        pub async fn accept(&mut self) -> std::io::Result<NamedPipeServer> {
            self.server.connect().await?;
            // Keep a listening instance available before handing off a client.
            let next = ServerOptions::new()
                .reject_remote_clients(true)
                .create(&self.path)?;
            Ok(std::mem::replace(&mut self.server, next))
        }
    }

    pub async fn connect(path: &Path) -> std::io::Result<NamedPipeClient> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match ClientOptions::new().open(path) {
                // ERROR_PIPE_BUSY: another client is being accepted.
                Err(err) if err.raw_os_error() == Some(231) && Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                result => return result,
            }
        }
    }
}

pub use platform::{Listener, connect};
