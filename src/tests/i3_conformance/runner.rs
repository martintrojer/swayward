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

/// What one unchanged i3 file produced.
struct I3Run {
    success: bool,
    stdout: String,
    stderr: String,
}

fn run_i3_test(test: &str) {
    let I3Run {
        success,
        stdout,
        stderr,
    } = run_i3_file(test);
    let rejected = rejected_commands(&stderr).collect::<Vec<_>>();
    let expected = expected_rejections(test)
        .iter()
        .map(|item| item.command)
        .collect::<Vec<_>>();
    let adapter_failed = stderr.contains("swayward xdotool adapter");
    let skips = tap_skips(&stdout);
    assert!(
        success
            && rejections_match(test, &rejected)
            && !adapter_failed
            && (!passing_tests().any(|green| green == test) || skips.is_empty()),
        "i3 test {test} failed, its xdotool adapter failed, its rejected commands changed, or a green file skipped assertions\nTAP failures:\n{}\nTAP skips: {skips:?}\nexpected rejections: {expected:?}\nactual rejections: {rejected:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        tap_failure_summary(&stdout, &stderr),
    );
}

/// Run one unchanged i3 file to completion and return its output without
/// judging it. Panics only when the file times out or the harness itself
/// fails.
fn run_i3_file(test: &str) -> I3Run {
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

    let scratch = I3Scratch::new();
    let handle = fixture.swayward().event_loop.clone();
    let ipc_socket = scratch.path("ipc.sock");
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(ipc_socket.clone())).unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.niri_state().ipc_refresh_layout();

    let control_path = scratch.path("control.sock");
    let control = UnixListener::bind(&control_path).unwrap();
    control.set_nonblocking(true).unwrap();

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
    let mut session = Session {
        test,
        client,
        loaded_config_source: None,
        scratch: &scratch,
        initially_floating: HashSet::new(),
    };
    loop {
        fixture.dispatch();
        match control.accept() {
            Ok((stream, _)) => handle_control(&mut fixture, &mut session, stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("test control accept failed: {error}"),
        }
        if let Some(status) = child.child_mut().try_wait().unwrap() {
            child.disarm();
            let (stdout, stderr) = child_output.finish();
            let stdout = String::from_utf8_lossy(&stdout).into_owned();
            eprint!("{stdout}");
            let stderr = String::from_utf8_lossy(&stderr).into_owned();
            if !stderr.is_empty() {
                eprint!("{stderr}");
            }
            return I3Run {
                success: status.success(),
                stdout,
                stderr,
            };
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
        pause_i3_poll();
    }
}

#[test]
fn i3_child_polling_yields_cpu_between_checks() {
    let started = Instant::now();
    for _ in 0..10 {
        pause_i3_poll();
    }
    assert!(started.elapsed() >= Duration::from_millis(5));
}

