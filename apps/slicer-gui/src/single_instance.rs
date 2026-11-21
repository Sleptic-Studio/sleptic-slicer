// Copyright (c) 2026 Sleptic
// SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
//! Per-user local launch hand-off. Never discard arguments without an acknowledgement.
use std::{
    io::{self, Read, Write},
    path::PathBuf,
};

const MAX_MESSAGE: usize = 1024 * 1024;
const VERSION: u32 = 1;

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct Message {
    version: u32,
    paths: Vec<PathBuf>,
}
fn write_message(mut writer: impl Write, paths: &[PathBuf]) -> io::Result<()> {
    let message = Message {
        version: VERSION,
        paths: paths.to_vec(),
    };
    let data = serde_json::to_vec(&message)?;
    if data.len() > MAX_MESSAGE {
        return Err(io::Error::other("Launch message too large"));
    }
    writer.write_all(&(data.len() as u32).to_le_bytes())?;
    writer.write_all(&data)?;
    writer.flush()
}
fn read_message(mut reader: impl Read) -> io::Result<Vec<PathBuf>> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_MESSAGE {
        return Err(io::Error::other("Launch message too large"));
    }
    let mut data = vec![0; length];
    reader.read_exact(&mut data)?;
    let message: Message = serde_json::from_slice(&data)?;
    if message.version != VERSION {
        return Err(io::Error::other("Unknown launch protocol"));
    }
    Ok(message.paths)
}

pub(crate) enum Launch {
    Forwarded,
    Local(Option<Server>),
}
/// Holds the per-user OS lock for the lifetime of the event loop.
pub(crate) struct Server {
    _lock: std::fs::File,
    name: String,
}
impl Server {
    pub(crate) fn listen(self, receive: impl Fn(Vec<PathBuf>) -> bool + Send + 'static) -> Self {
        let name = self.name.clone();
        std::thread::spawn(move || platform::listen(&name, receive));
        self
    }
}
/// Verification sessions (and `--new-instance`) intentionally own
/// independent windows and never hand off.
pub(crate) fn begin(options: &crate::launch_options::LaunchOptions) -> Launch {
    if options.new_instance || options.screenshot.is_some() || options.dump_state.is_some() {
        return Launch::Local(None);
    }
    let Some(dir) = sc_app_paths::project_dirs().map(|d| d.data_local_dir().join("instance"))
    else {
        return Launch::Local(None);
    };
    resolve_claim(claim(&dir, &options.open))
}
fn resolve_claim(result: io::Result<Launch>) -> Launch {
    match result {
        Ok(result) => result,
        Err(error) => {
            eprintln!("Launch hand-off unavailable; opening files here: {error}");
            Launch::Local(None)
        }
    }
}
fn claim(dir: &std::path::Path, paths: &[PathBuf]) -> io::Result<Launch> {
    use std::hash::{Hash, Hasher};
    std::fs::create_dir_all(dir)?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("launch.lock"))?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut hash);
    let name = format!("sleptic-slicer-{:016x}", hash.finish());
    if lock.try_lock().is_ok() {
        return Ok(Launch::Local(Some(Server { _lock: lock, name })));
    }
    // Absolutize relative paths in the sender's working directory.
    let cwd = std::env::current_dir()?;
    let paths: Vec<_> = paths
        .iter()
        .map(|p| {
            if p.is_absolute() {
                p.clone()
            } else {
                cwd.join(p)
            }
        })
        .collect();
    platform::forward(&name, &paths)?;
    Ok(Launch::Forwarded)
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateNamedPipeW(
            name: *const u16,
            mode: u32,
            pipe_mode: u32,
            instances: u32,
            out_size: u32,
            in_size: u32,
            timeout: u32,
            security: *const std::ffi::c_void,
        ) -> RawHandle;
        fn ConnectNamedPipe(pipe: RawHandle, overlapped: *mut std::ffi::c_void) -> i32;
        fn FlushFileBuffers(handle: RawHandle) -> i32;
        fn GetNamedPipeServerProcessId(handle: RawHandle, process_id: *mut u32) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn AllowSetForegroundWindow(process_id: u32) -> i32;
    }
    fn path(name: &str) -> String {
        format!(r"\\.\pipe\{name}")
    }
    fn wide(path: &str) -> Vec<u16> {
        path.encode_utf16().chain(Some(0)).collect()
    }
    pub(super) fn forward(name: &str, paths: &[PathBuf]) -> io::Result<()> {
        let path = path(name);
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        // Attempt the connection itself: pipe discovery can report a missing
        // instance even when opening it succeeds. Retrying the open also handles
        // another client taking an available instance before this one connects.
        let mut file = loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&path)
            {
                Ok(file) => break file,
                Err(error)
                    if matches!(error.raw_os_error(), Some(2 | 231))
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("Opening launch pipe: {error}"),
                    ));
                }
            }
        };
        let mut process_id = 0u32;
        // SAFETY: live pipe and valid writable PID pointer. Grant only its
        // server the foreground permission carried by this user launch.
        if unsafe { GetNamedPipeServerProcessId(file.as_raw_handle(), &mut process_id) } != 0 {
            // SAFETY: PID was supplied by the local pipe server query.
            unsafe {
                AllowSetForegroundWindow(process_id);
            }
        }
        write_message(&mut file, paths)?;
        // Read the one-byte acknowledgement with an ordinary blocking read on
        // a helper thread, bounded by recv_timeout. Switching the client to
        // PIPE_NOWAIT for polling failed intermittently on a real desktop
        // (SetNamedPipeHandleState -> os error 231), so most second launches
        // fell back to their own window instead of handing off.
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut ack = [0];
            let _ = sender.send(file.read_exact(&mut ack).map(|()| ack));
        });
        let ack = match receiver.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(result) => result?,
            Err(_) => return Err(io::Error::other("Launch acknowledgement missing")),
        };
        if ack != [1] {
            return Err(io::Error::other("Launch was not accepted"));
        }
        Ok(())
    }
    pub(super) fn listen(name: &str, receive: impl Fn(Vec<PathBuf>) -> bool) {
        let wide = wide(&path(name));
        loop {
            // PIPE_ACCESS_DUPLEX, byte mode, PIPE_REJECT_REMOTE_CLIENTS.
            // SAFETY: valid terminated name, null default security, owned returned handle.
            let handle = unsafe {
                CreateNamedPipeW(wide.as_ptr(), 3, 8, 1, 4096, 4096, 1500, std::ptr::null())
            };
            if handle as isize == -1 {
                eprintln!("Launch pipe: {}", io::Error::last_os_error());
                return;
            }
            // SAFETY: unique valid handle is transferred to File for RAII closure.
            let mut file = unsafe { std::fs::File::from_raw_handle(handle) };
            // SAFETY: handle stays alive in file, synchronous call uses null overlapped.
            let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) } != 0
                || io::Error::last_os_error().raw_os_error() == Some(535);
            if connected && let Ok(paths) = read_message(&mut file) {
                let accepted = receive(paths);
                if file.write_all(&[u8::from(accepted)]).is_ok() {
                    // SAFETY: file owns this valid synchronous pipe handle.
                    unsafe {
                        FlushFileBuffers(handle);
                    }
                }
            }
        }
    }
}
#[cfg(not(windows))]
mod platform {
    use super::*;
    pub(super) fn forward(_: &str, _: &[PathBuf]) -> io::Result<()> {
        Err(io::Error::other(
            "Launch hand-off is not available on this platform",
        ))
    }
    pub(super) fn listen(_: &str, _: impl Fn(Vec<PathBuf>) -> bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_preserves_multiple_unicode_paths_and_rejects_truncation() {
        let paths = vec![
            PathBuf::from("C:/models/a b.stl"),
            PathBuf::from("\u{6a21}\u{578b}.3mf"),
        ];
        let mut data = Vec::new();
        write_message(&mut data, &paths).unwrap();
        assert_eq!(read_message(&data[..]).unwrap(), paths);
        assert!(read_message(&data[..data.len() - 1]).is_err());
        assert!(read_message(&u32::MAX.to_le_bytes()[..]).is_err());
        let mut version = Vec::new();
        let body = br#"{"version":2,"paths":[]}"#;
        version.extend_from_slice(&(body.len() as u32).to_le_bytes());
        version.extend_from_slice(body);
        assert!(read_message(&version[..]).is_err());
    }
    #[test]
    fn unavailable_receiver_preserves_local_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let paths = vec![PathBuf::from("model.stl")];
        let Launch::Local(Some(_server)) = claim(dir.path(), &paths).unwrap() else {
            panic!("first owner");
        };
        assert!(claim(dir.path(), &paths).is_err());
        assert_eq!(paths, vec![PathBuf::from("model.stl")]);
    }
    #[cfg(windows)]
    #[test]
    fn launch_server_accepts_successive_batches() {
        let dir = tempfile::tempdir().unwrap();
        let Launch::Local(Some(server)) = claim(dir.path(), &[]).unwrap() else {
            panic!("first owner");
        };
        let (send, receive) = std::sync::mpsc::channel();
        let _server = server.listen(move |paths| {
            send.send(paths).unwrap();
            true
        });
        for name in ["first.stl", "second.obj", "third.3mf"] {
            let paths = vec![std::env::current_dir().unwrap().join(name)];
            assert!(matches!(claim(dir.path(), &paths), Ok(Launch::Forwarded)));
            assert_eq!(
                receive
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap(),
                paths
            );
        }
    }
    #[cfg(windows)]
    #[test]
    fn rejected_handoff_preserves_local_paths() {
        let dir = tempfile::tempdir().unwrap();
        let Launch::Local(Some(server)) = claim(dir.path(), &[]).unwrap() else {
            panic!("first owner");
        };
        let _server = server.listen(|_| false);
        let paths = vec![std::env::current_dir().unwrap().join("keep-local.stl")];
        let error = claim(dir.path(), &paths).err().expect("rejected handoff");
        assert_eq!(error.to_string(), "Launch was not accepted");
        assert!(matches!(resolve_claim(Err(error)), Launch::Local(None)));
        assert!(paths[0].ends_with("keep-local.stl"));
    }
    #[cfg(windows)]
    #[test]
    fn named_pipe_handoff_or_os_denial_retains_local_files() {
        let dir = tempfile::tempdir().unwrap();
        let paths = vec![
            std::env::current_dir().unwrap().join("one.stl"),
            std::env::current_dir().unwrap().join("two.obj"),
        ];
        let Launch::Local(Some(server)) = claim(dir.path(), &[]).unwrap() else {
            panic!("first owner")
        };
        let (send, receive) = std::sync::mpsc::channel();
        let _server = server.listen(move |paths| {
            send.send(paths).unwrap();
            true
        });
        let mut forwarded = false;
        for _ in 0..10 {
            let result = claim(dir.path(), &paths);
            if let Err(ref error) = result
                && error.kind() == io::ErrorKind::PermissionDenied
            {
                eprintln!(
                    "Native pipe open denied by OS: {error}; verified local fallback retains {} paths",
                    paths.len()
                );
                assert!(matches!(resolve_claim(result), Launch::Local(None)));
                assert_eq!(paths.len(), 2);
                return;
            }
            if matches!(result, Ok(Launch::Forwarded)) {
                forwarded = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(forwarded);
        assert_eq!(
            receive
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            paths
        );
    }
}
