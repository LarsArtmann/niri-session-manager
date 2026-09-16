//! Async access to the niri IPC socket.
//!
//! The niri-ipc socket is blocking I/O; every request here runs on tokio's
//! blocking pool so callers never occupy a runtime worker (a current-thread
//! runtime would otherwise serialize entirely behind one call). All
//! request/reply I/O carries a read/write timeout so a wedged niri (accepts
//! connections but never replies) cannot park blocking-pool threads forever —
//! a parked blocking task would otherwise hang the whole process at runtime
//! drop, even after the async side already gave up (observed live 2026-09-15).

use anyhow::{Context, Result};
use niri_ipc::{Reply, Request, Response, Window, Workspace};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;
use tokio::task::spawn_blocking;

/// Bounded read/write timeout for one-shot request/reply IPC. Generous for a
/// healthy compositor (replies are immediate on a local Unix socket), short
/// enough that a wedged niri surfaces as an IPC error instead of a hang.
pub const IPC_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Opens a connection to the niri IPC socket, returning the stream and a
/// clone that can shut the connection down from another thread.
pub fn open_niri_socket() -> std::io::Result<(UnixStream, UnixStream)> {
    let socket_path = std::env::var_os(niri_ipc::socket::SOCKET_PATH_ENV).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "NIRI_SOCKET is not set, are you running this within niri?",
        )
    })?;
    let stream = UnixStream::connect(socket_path)?;
    let shutdown_handle = stream.try_clone()?;
    Ok((stream, shutdown_handle))
}

/// Sends one JSON-line request and reads the JSON-line reply under
/// [`IPC_REQUEST_TIMEOUT`]. The read timeout stays set on the stream: callers
/// that turn the connection into a long-lived event stream must clear it
/// (events legitimately arrive minutes apart).
pub fn request_reply(
    stream: &mut BufReader<UnixStream>,
    request: &Request,
) -> std::io::Result<Reply> {
    stream
        .get_mut()
        .set_read_timeout(Some(IPC_REQUEST_TIMEOUT))?;
    stream
        .get_mut()
        .set_write_timeout(Some(IPC_REQUEST_TIMEOUT))?;
    let mut buf = serde_json::to_string(request).map_err(std::io::Error::other)?;
    buf.push('\n');
    stream.get_mut().write_all(buf.as_bytes())?;
    buf.clear();
    stream.read_line(&mut buf)?;
    serde_json::from_str(&buf).map_err(std::io::Error::other)
}

/// Sends one request on a fresh connection and returns the successful
/// response, mapping protocol and transport failures to errors. Runs on the
/// blocking pool with bounded I/O timeouts.
pub async fn niri_send(request: Request) -> Result<Response> {
    spawn_blocking(move || {
        let (stream, _shutdown_handle) =
            open_niri_socket().context("Failed to connect to Niri IPC socket")?;
        let mut stream = BufReader::new(stream);
        let reply =
            request_reply(&mut stream, &request).context("Failed to communicate with Niri IPC")?;
        match reply {
            Reply::Ok(response) => Ok(response),
            Reply::Err(error_msg) => anyhow::bail!("Niri IPC returned an error: {error_msg}"),
        }
    })
    .await
    .context("Niri IPC task join error")?
}

pub async fn get_niri_windows() -> Result<Vec<Window>> {
    match niri_send(Request::Windows).await? {
        Response::Windows(windows) => Ok(windows),
        _ => anyhow::bail!("Expected Windows response from Niri"),
    }
}

pub async fn get_niri_workspaces() -> Result<Vec<Workspace>> {
    match niri_send(Request::Workspaces).await? {
        Response::Workspaces(workspaces) => Ok(workspaces),
        _ => anyhow::bail!("Expected Workspaces response from Niri"),
    }
}
