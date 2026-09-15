//! Async access to the niri IPC socket.
//!
//! The niri-ipc socket is blocking I/O; every request here runs on tokio's
//! blocking pool so callers never occupy a runtime worker (a current-thread
//! runtime would otherwise serialize entirely behind one call).

use anyhow::{Context, Result};
use niri_ipc::{socket::Socket, Reply, Request, Response, Window, Workspace};
use tokio::task::spawn_blocking;

pub async fn niri_send(request: Request) -> Result<Response> {
    spawn_blocking(move || {
        let mut socket = Socket::connect().context("Failed to connect to Niri IPC socket")?;
        let reply = socket
            .send(request)
            .context("Failed to communicate with Niri IPC")?;
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
