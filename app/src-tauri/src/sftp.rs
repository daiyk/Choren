//! File transfer to the robot over SSH/SFTP: list a folder, go up, upload, download, create a
//! folder, delete; one transfer at a time, with progress and Cancel. Independent of the libqi connection.

use russh::client;
use russh_sftp::client::SftpSession;
use serde_json::{json, Value as Json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

const CHUNK: usize = 32 * 1024;

#[derive(Default)]
pub struct SftpState {
    remote: Mutex<Option<Remote>>,
    busy: AtomicBool,
    cancel: AtomicBool,
}

struct Remote {
    sftp: Arc<SftpSession>,
    _ssh: client::Handle<Accept>,
}

/// Robots are reflashed and renamed often, so any host key is accepted (no known_hosts).
struct Accept;

impl client::Handler for Accept {
    type Error = russh::Error;
    async fn check_server_key(&mut self, _: &russh::keys::PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

async fn session(state: &SftpState) -> Result<Arc<SftpSession>, String> {
    state.remote.lock().await.as_ref().map(|r| r.sftp.clone()).ok_or_else(|| "Not connected to the robot's files.".into())
}

/// Opens SSH (port 22) with a password and starts SFTP; returns the home folder.
#[tauri::command]
pub async fn sftp_connect(state: State<'_, SftpState>, host: String, user: String, password: String) -> Result<String, String> {
    state.remote.lock().await.take();
    let config = Arc::new(client::Config { inactivity_timeout: None, ..Default::default() });
    let connect = client::connect(config, (host.as_str(), 22), Accept);
    let mut ssh = tokio::time::timeout(std::time::Duration::from_secs(5), connect).await.map_err(|_| format!("No SSH answer from {host}"))?.map_err(|e| format!("SSH connection error: {e}"))?;
    if !ssh.authenticate_password(user, password).await.map_err(err)?.success() {
        return Err("SSH authentication error.".into());
    }
    let channel = ssh.channel_open_session().await.map_err(err)?;
    channel.request_subsystem(true, "sftp").await.map_err(|_| "Unable to open a SFTP channel.")?;
    let sftp = SftpSession::new(channel.into_stream()).await.map_err(|_| "Unable to open a SFTP channel.")?;
    let home = sftp.canonicalize(".").await.map_err(err)?;
    *state.remote.lock().await = Some(Remote { sftp: Arc::new(sftp), _ssh: ssh });
    Ok(home)
}

#[tauri::command]
pub async fn sftp_disconnect(state: State<'_, SftpState>) -> Result<(), String> {
    state.cancel.store(true, Ordering::Relaxed);
    if let Some(r) = state.remote.lock().await.take() {
        let _ = r.sftp.close().await;
    }
    Ok(())
}

/// A folder's entries, folders first then by name.
#[tauri::command]
pub async fn sftp_list(state: State<'_, SftpState>, path: String) -> Result<Json, String> {
    list(&*session(&state).await?, path).await
}

async fn list(sftp: &SftpSession, path: String) -> Result<Json, String> {
    let path = sftp.canonicalize(path).await.map_err(err)?;
    let mut entries: Vec<(bool, String, u64)> = sftp
        .read_dir(path.as_str())
        .await
        .map_err(|e| format!("An error happened while listing a directory content: {e}"))?
        .filter(|e| e.file_name() != "." && e.file_name() != "..")
        .map(|e| (e.file_type().is_dir(), e.file_name(), e.metadata().len()))
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    let entries: Vec<Json> = entries.into_iter().map(|(dir, name, size)| json!({"name": name, "dir": dir, "size": size})).collect();
    Ok(json!({"path": path, "entries": entries}))
}

#[tauri::command]
pub async fn sftp_mkdir(state: State<'_, SftpState>, path: String) -> Result<(), String> {
    session(&state).await?.create_dir(path).await.map_err(err)
}

/// Deletes a file, or a folder once it is empty (SFTP's `rmdir`).
#[tauri::command]
pub async fn sftp_remove(state: State<'_, SftpState>, path: String, dir: bool) -> Result<(), String> {
    let sftp = session(&state).await?;
    if dir { sftp.remove_dir(path).await.map_err(|e| format!("Cannot delete the folder (it must be empty): {e}")) } else { sftp.remove_file(path).await.map_err(err) }
}

#[tauri::command]
pub fn sftp_cancel(state: State<'_, SftpState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

/// Copies `from` to `to` in chunks, reporting `(done, total)`; stops on Cancel.
async fn copy(cancel: &AtomicBool, progress: &mut impl FnMut(u64, u64), mut from: impl tokio::io::AsyncRead + Unpin, mut to: impl tokio::io::AsyncWrite + Unpin, total: u64) -> Result<(), String> {
    let mut buf = vec![0; CHUNK];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Transfer canceled.".into());
        }
        let n = from.read(&mut buf).await.map_err(|e| format!("Error during transfer: {e}."))?;
        if n == 0 {
            break;
        }
        to.write_all(&buf[..n]).await.map_err(|e| format!("Error during transfer: {e}."))?;
        done += n as u64;
        progress(done, total);
    }
    to.shutdown().await.map_err(|e| format!("Error during transfer: {e}."))
}

/// One transfer at a time ("Already doing a transfer."); the flag clears when the transfer ends.
struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

async fn begin(state: &SftpState) -> Result<(Arc<SftpSession>, Busy<'_>), String> {
    let sftp = session(state).await?;
    if state.busy.swap(true, Ordering::AcqRel) {
        return Err("Already doing a transfer.".into());
    }
    state.cancel.store(false, Ordering::Relaxed);
    Ok((sftp, Busy(&state.busy)))
}

/// `sftp://progress` `[done, total]` for the panel.
fn emitter(app: &AppHandle) -> impl FnMut(u64, u64) + '_ {
    move |done, total| {
        let _ = app.emit("sftp://progress", json!([done, total]));
    }
}

/// Uploads a local file (picked through the file dialog) to `remote`.
#[tauri::command]
pub async fn sftp_upload(app: AppHandle, state: State<'_, SftpState>, local: String, remote: String) -> Result<(), String> {
    let path = crate::project::granted(&app, &local)?;
    let (sftp, _busy) = begin(&state).await?;
    upload(&sftp, &state.cancel, &path, &remote, &mut emitter(&app)).await
}

/// Downloads `remote` to a local file chosen through the save dialog.
#[tauri::command]
pub async fn sftp_download(app: AppHandle, state: State<'_, SftpState>, remote: String, local: String) -> Result<(), String> {
    let path = crate::project::granted(&app, &local)?;
    let (sftp, _busy) = begin(&state).await?;
    download(&sftp, &state.cancel, &remote, &path, &mut emitter(&app)).await
}

/// A canceled or failed upload leaves no partial file on the robot.
async fn upload(sftp: &SftpSession, cancel: &AtomicBool, local: &Path, remote: &str, progress: &mut impl FnMut(u64, u64)) -> Result<(), String> {
    let file = tokio::fs::File::open(local).await.map_err(|_| format!("The file to upload ({}) does not exist.", local.display()))?;
    let total = file.metadata().await.map_err(err)?.len();
    let target = sftp.create(remote).await.map_err(err)?;
    let result = copy(cancel, progress, file, target, total).await;
    if result.is_err() {
        let _ = sftp.remove_file(remote).await;
    }
    result
}

/// A canceled or failed download leaves no partial local file.
async fn download(sftp: &SftpSession, cancel: &AtomicBool, remote: &str, local: &Path, progress: &mut impl FnMut(u64, u64)) -> Result<(), String> {
    let source = sftp.open(remote).await.map_err(err)?;
    let total = source.metadata().await.map_err(err)?.len();
    let file = tokio::fs::File::create(local).await.map_err(|e| format!("Cannot write {}: {e}", local.display()))?;
    let result = copy(cancel, progress, source, file, total).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(local).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OpenSSH's own SFTP server (what a NAO's sshd runs for the `sftp` subsystem), over stdio.
    #[tokio::test]
    async fn lists_uploads_downloads_and_cancels_against_openssh_sftp_server() {
        let server = std::path::Path::new("/usr/libexec/sftp-server");
        if !server.exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("choren-sftp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let mut child = tokio::process::Command::new(server).current_dir(&dir).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).kill_on_drop(true).spawn().unwrap();
        let stream = tokio::io::join(child.stdout.take().unwrap(), child.stdin.take().unwrap());
        let sftp = SftpSession::new(stream).await.unwrap();
        let root = dir.to_str().unwrap();

        let data: Vec<u8> = (0..200_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let local = dir.join("local.bin");
        std::fs::write(&local, &data).unwrap();
        sftp.create_dir(format!("{root}/sub")).await.unwrap();
        let (cancel, mut seen) = (AtomicBool::new(false), vec![]);
        upload(&sftp, &cancel, &local, &format!("{root}/sub/B.bin"), &mut |d, t| seen.push((d, t))).await.unwrap();
        assert_eq!(seen.last(), Some(&(200_000, 200_000)));
        assert!(seen.len() > 1);
        std::fs::write(dir.join("a.txt"), "x").unwrap();

        let listed = list(&sftp, ".".into()).await.unwrap();
        assert_eq!(listed["path"], root);
        let names: Vec<_> = listed["entries"].as_array().unwrap().iter().map(|e| (e["name"].as_str().unwrap().to_string(), e["dir"].as_bool().unwrap())).collect();
        assert_eq!(names, [("sub".into(), true), ("a.txt".into(), false), ("local.bin".into(), false)]);
        assert_eq!(list(&sftp, format!("{root}/sub")).await.unwrap()["entries"][0], json!({"name": "B.bin", "dir": false, "size": 200_000}));

        let back = dir.join("back.bin");
        download(&sftp, &cancel, &format!("{root}/sub/B.bin"), &back, &mut |_, _| {}).await.unwrap();
        assert_eq!(std::fs::read(&back).unwrap(), data);

        cancel.store(true, Ordering::Relaxed);
        assert_eq!(upload(&sftp, &cancel, &local, &format!("{root}/sub/C.bin"), &mut |_, _| {}).await, Err("Transfer canceled.".into()));
        assert!(!sftp.try_exists(format!("{root}/sub/C.bin")).await.unwrap());
        assert!(download(&sftp, &cancel, &format!("{root}/sub/B.bin"), &dir.join("partial.bin"), &mut |_, _| {}).await.is_err());
        assert!(!dir.join("partial.bin").exists());

        assert!(sftp.remove_dir(format!("{root}/sub")).await.is_err());
        sftp.remove_file(format!("{root}/sub/B.bin")).await.unwrap();
        sftp.remove_dir(format!("{root}/sub")).await.unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
