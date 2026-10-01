use std::os::unix::fs::FileTypeExt as _;

use super::transport::{queue_ipc_message, socket_dir_from};
use super::*;

#[test]
fn test_socket_paths_are_unique_and_avoid_tmpfs() {
    let first = test_socket_path("socket");
    let second = test_socket_path("socket");
    assert_ne!(first, second);
    assert_eq!(first.parent(), Some(std::path::Path::new("/var/tmp")));
}

#[test]
fn default_socket_path_uses_runtime_dir_and_falls_back_to_tmp() {
    let runtime = PathBuf::from("/run/user/1234");
    assert_eq!(socket_dir_from(Some(runtime.clone())), runtime);
    assert_eq!(socket_dir_from(None), env::temp_dir());
    assert_eq!(
        default_socket_path(runtime, OsStr::new("wayland-7"), 42, 3),
        PathBuf::from("/run/user/1234/swayward-ipc.wayland-7.42.3.sock")
    );
}

#[test]
fn socket_path_honors_only_a_nonexistent_swaysock() {
    let root = test_socket_path("socket-path");
    let requested = root.join("requested.sock");
    let fallback = root.join("fallback.sock");
    std::fs::create_dir_all(&root).unwrap();

    assert_eq!(
        select_socket_path(fallback.clone(), Some(requested.clone())),
        requested
    );
    let occupied = UnixListener::bind(&requested).unwrap();
    assert_eq!(
        select_socket_path(fallback.clone(), Some(requested.clone())),
        fallback
    );
    assert!(UnixStream::connect(&requested).is_ok());

    drop(occupied);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn binding_removes_a_stale_socket_and_drop_cleans_up() {
    let root = test_socket_path("stale-socket");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("ipc.sock");
    drop(UnixListener::bind(&path).unwrap());

    let event_loop = calloop::EventLoop::<State>::try_new().unwrap();
    let server = IpcServer::start_at(&event_loop.handle(), Some(path.clone())).unwrap();
    assert!(path.metadata().unwrap().file_type().is_socket());
    drop(server);
    assert!(!path.exists());

    let server = IpcServer::start_at(&event_loop.handle(), Some(path.clone())).unwrap();
    assert!(path.metadata().unwrap().file_type().is_socket());
    drop(server);
    assert!(!path.exists());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn overlong_socket_path_fails_instead_of_truncating() {
    let root = test_socket_path("overlong-socket");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("x".repeat(108));
    assert!(bind_listener(&path).is_err());
    assert!(!path.exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn write_buffer_matches_sways_doubling_limit() {
    let mut buffer = Vec::new();
    let mut size = INITIAL_WRITE_BUFFER_SIZE;
    queue_ipc_message(&mut buffer, &mut size, &[0; 100]).unwrap();
    assert_eq!(size, 128);
    queue_ipc_message(&mut buffer, &mut size, &[0; 28]).unwrap();
    assert_eq!(size, 256);

    buffer.resize(2_097_151, 0);
    size = 2_097_152;
    assert!(queue_ipc_message(&mut buffer, &mut size, &[0]).is_err());
    assert_eq!(size, 4_194_304);
}
