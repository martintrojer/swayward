fn tap_failure_summary(stdout: &str, stderr: &str) -> String {
    stdout
        .lines()
        .filter(|line| line.starts_with("not ok "))
        .chain(
            stderr
                .lines()
                .filter(|line| line.starts_with("#   Failed test") || line.starts_with("#   at ")),
        )
        .collect::<Vec<_>>()
        .join("\n")
}

fn tap_skips(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| line.starts_with("ok ") && line.contains("# skip"))
        .collect()
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("non-string panic payload")
}

fn with_test_context(test: &str, run: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        panic!(
            "i3 test {test} panicked: {}",
            panic_message(payload.as_ref())
        );
    }
}

fn run_i3_test_with_context(test: &str) {
    with_test_context(test, || run_i3_test(test));
}

fn collect_test_failures<'a>(
    tests: impl IntoIterator<Item = &'a str>,
    mut run: impl FnMut(&str),
) -> Vec<(String, String)> {
    tests
        .into_iter()
        .filter_map(|test| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(test)))
                .err()
                .map(|payload| (test.to_owned(), panic_message(payload.as_ref()).to_owned()))
        })
        .collect()
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn child_mut(&mut self) -> &mut Child {
        self.0.as_mut().unwrap()
    }

    fn disarm(&mut self) {
        self.0.take();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct ChildOutput {
    stdout: thread::JoinHandle<Vec<u8>>,
    stderr: thread::JoinHandle<Vec<u8>>,
}

impl ChildOutput {
    fn new(child: &mut Child) -> Self {
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        Self {
            stdout: thread::spawn(move || {
                let mut bytes = Vec::new();
                stdout.read_to_end(&mut bytes).unwrap();
                bytes
            }),
            stderr: thread::spawn(move || {
                let mut bytes = Vec::new();
                stderr.read_to_end(&mut bytes).unwrap();
                bytes
            }),
        }
    }

    fn finish(self) -> (Vec<u8>, Vec<u8>) {
        (self.stdout.join().unwrap(), self.stderr.join().unwrap())
    }
}

fn run_i3_test(test: &str) {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 0.;
    config.layout.border.off = false;
    configure_client_state_oracle(&mut config, test);
    config.input.focus_follows_mouse = Some(swayward_config::input::FocusFollowsMouse {
        mode: swayward_config::input::FocusFollowsMouseMode::Yes,
        max_scroll_amount: None,
    });
    config.animations.window_movement.0.off = true;
    config.animations.window_resize.anim.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    let client = fixture.add_client();

    let handle = fixture.swayward().event_loop.clone();
    let ipc_dir = socket_path("ipc");
    std::fs::create_dir(&ipc_dir).unwrap();
    let ipc_socket = ipc_dir.join("ipc.sock");
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(ipc_socket.clone())).unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.niri_state().ipc_refresh_layout();

    let control_path = socket_path("control");
    let control = UnixListener::bind(&control_path).unwrap();
    control.set_nonblocking(true).unwrap();
    // Remove the socket even when a test panics or times out. Without this the
    // whole suite leaks one file per conformance test per run, and a run left
    // over 7000 of them in the temp directory. Unix socket paths are limited to
    // about 108 bytes, so an accumulating temp directory eventually makes bind
    // fail in whichever file happens to run next.
    struct Scratch {
        files: Vec<PathBuf>,
        dirs: Vec<PathBuf>,
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            for path in &self.files {
                let _ = std::fs::remove_file(path);
            }
            // remove_dir_all: the IPC server's socket still sits inside the
            // directory when this runs, so a plain remove_dir fails and every
            // conformance test left one directory behind (about 16,000 after
            // a day of gate runs).
            for path in &self.dirs {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
    let mut scratch = Scratch {
        files: vec![control_path.clone()],
        dirs: vec![ipc_dir],
    };

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let oracle = oracle_i3_dir();
    let mut child = Command::new("perl")
        .arg(format!("-I{}", root.join("tests/i3/lib").display()))
        .arg(format!("-I{}", oracle.join("lib").display()))
        .arg(oracle.join("t").join(test))
        .env("I3SOCK", &ipc_socket)
        .env("SWAYWARD_TEST_CONTROL", &control_path)
        .env("SWAYWARD_I3_TEST", test)
        .env(
            "PATH",
            format!(
                "{}:{}",
                root.join("tests/i3/bin").display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let child_output = ChildOutput::new(&mut child);
    let mut child = ChildGuard::new(child);

    let started = Instant::now();
    // Generous enough that a cold build cache and a loaded machine cannot trip
    // it: 132-move-workspace.t runs 160 assertions in ~14s warm but has been
    // measured at 33s cold, and the suite runs these files in parallel. A
    // genuinely hung test still fails, just later.
    let deadline = started + Duration::from_secs(180);
    let mut loaded_config_source = None;
    let mut initially_floating = HashSet::new();
    loop {
        fixture.dispatch();
        match control.accept() {
            Ok((stream, _)) => handle_control(
                &mut fixture,
                client,
                &mut loaded_config_source,
                &mut scratch.files,
                &mut initially_floating,
                stream,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("test control accept failed: {error}"),
        }
        if let Some(status) = child.child_mut().try_wait().unwrap() {
            child.disarm();
            let (stdout, stderr) = child_output.finish();
            let stdout = String::from_utf8_lossy(&stdout);
            eprint!("{stdout}");
            let stderr = String::from_utf8_lossy(&stderr);
            if !stderr.is_empty() {
                eprint!("{stderr}");
            }
            let rejected = rejected_commands(&stderr).collect::<Vec<_>>();
            let expected = expected_rejections(test)
                .iter()
                .map(|item| item.command)
                .collect::<Vec<_>>();
            let adapter_failed = stderr.contains("swayward xdotool adapter");
            let skips = tap_skips(&stdout);
            assert!(
                status.success()
                    && rejections_match(test, &rejected)
                    && !adapter_failed
                    && (!passing_tests().any(|green| green == test) || skips.is_empty()),
                "i3 test {test} failed, its xdotool adapter failed, its rejected commands changed, or a green file skipped assertions\nTAP failures:\n{}\nTAP skips: {skips:?}\nexpected rejections: {expected:?}\nactual rejections: {rejected:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                tap_failure_summary(&stdout, &stderr),
            );
            break;
        }
        if Instant::now() >= deadline {
            child.child_mut().kill().unwrap();
            child.child_mut().wait().unwrap();
            child.disarm();
            let (stdout, stderr) = child_output.finish();
            let stdout = String::from_utf8_lossy(&stdout);
            let stderr = String::from_utf8_lossy(&stderr);
            panic!(
                "i3 test {test} timed out after {:?}\nTAP failures:\n{}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                started.elapsed(),
                tap_failure_summary(&stdout, &stderr),
            );
        }
        thread::yield_now();
    }
}

