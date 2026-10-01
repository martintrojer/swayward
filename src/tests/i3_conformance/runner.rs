use super::*;

pub(super) fn tap_failure_summary(stdout: &str, stderr: &str) -> String {
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

pub(super) fn tap_skips(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| line.starts_with("ok ") && line.contains("# skip"))
        .collect()
}

pub(super) fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("non-string panic payload")
}

pub(super) fn with_test_context(test: &str, run: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        panic!(
            "i3 test {test} panicked: {}",
            panic_message(payload.as_ref())
        );
    }
}

pub(super) fn run_i3_test_with_context(test: &str, is_green: bool) {
    with_test_context(test, || {
        let run = run_i3_file(test);
        if let Err(verdict) = verdict(test, is_green, &run) {
            panic!("{}", verdict.message(test, &run));
        }
    });
}

pub(super) fn collect_test_failures<'a>(
    tests: impl IntoIterator<Item = (&'a str, bool)>,
    mut run: impl FnMut(&str, bool),
) -> Vec<(String, String)> {
    tests
        .into_iter()
        .filter_map(|(test, is_green)| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(test, is_green)))
                .err()
                .map(|payload| (test.to_owned(), panic_message(payload.as_ref()).to_owned()))
        })
        .collect()
}

pub(super) struct ChildGuard(Option<Child>);

impl ChildGuard {
    pub(super) fn new(child: Child) -> Self {
        Self(Some(child))
    }

    pub(super) fn child_mut(&mut self) -> &mut Child {
        self.0.as_mut().unwrap()
    }

    pub(super) fn disarm(&mut self) {
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

pub(super) struct ChildOutput {
    pub(super) stdout: thread::JoinHandle<Vec<u8>>,
    pub(super) stderr: thread::JoinHandle<Vec<u8>>,
}

impl ChildOutput {
    pub(super) fn new(child: &mut Child) -> Self {
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

    pub(super) fn finish(self) -> (Vec<u8>, Vec<u8>) {
        (self.stdout.join().unwrap(), self.stderr.join().unwrap())
    }
}

/// What one unchanged i3 file produced.
pub(super) struct I3Run {
    pub(super) success: bool,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

#[derive(Debug, PartialEq)]
pub(super) enum Verdict {
    TapFailed,
    AdapterFailed,
    RejectionsChanged {
        expected: Vec<String>,
        actual: Vec<String>,
    },
    GreenFileSkipped(Vec<String>),
}

impl Verdict {
    fn message(&self, test: &str, run: &I3Run) -> String {
        match self {
            Self::TapFailed => format!(
                "i3 test {test} failed\nTAP failures:\n{}\nstdout:\n{}\nstderr:\n{}",
                tap_failure_summary(&run.stdout, &run.stderr), run.stdout, run.stderr
            ),
            Self::AdapterFailed => format!("i3 test {test} xdotool adapter failed\nstderr:\n{}", run.stderr),
            Self::RejectionsChanged { expected, actual } => format!(
                "i3 test {test} rejected commands changed\nexpected: {expected:?}\nactual: {actual:?}"
            ),
            Self::GreenFileSkipped(skips) => format!("green i3 test {test} skipped assertions: {skips:?}"),
        }
    }
}

pub(super) fn verdict(test: &str, is_green: bool, run: &I3Run) -> Result<(), Verdict> {
    if !run.success {
        return Err(Verdict::TapFailed);
    }
    if run.stderr.contains("swayward xdotool adapter") {
        return Err(Verdict::AdapterFailed);
    }
    let actual = rejected_commands(&run.stderr)
        .map(|command| command.to_owned())
        .collect::<Vec<_>>();
    let expected = expected_rejections(test)
        .iter()
        .map(|item| item.command.to_owned())
        .collect::<Vec<_>>();
    if !rejections_match(test, &rejected_commands(&run.stderr).collect::<Vec<_>>()) {
        return Err(Verdict::RejectionsChanged { expected, actual });
    }
    let skips = tap_skips(&run.stdout)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if is_green && !skips.is_empty() {
        return Err(Verdict::GreenFileSkipped(skips));
    }
    Ok(())
}

/// Run one unchanged i3 file to completion and return its output without
/// judging it. Panics only when the file times out or the harness itself
/// fails.
pub(super) fn run_i3_file(test: &str) -> I3Run {
    let mut config = swayward_config::Config::default();
    apply_harness_policy(&mut config, None, test);
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
    let skip_adaptations = coverage_skip_adaptations(test);
    let adaptations = coverage_adaptations(test);
    let mut child = Command::new("perl")
        .arg(format!("-I{}", root.join("tests/i3/lib").display()))
        .arg(format!("-I{}", oracle.join("lib").display()))
        .arg(oracle.join("t").join(test))
        .env("I3SOCK", &ipc_socket)
        .env("SWAYWARD_TEST_CONTROL", &control_path)
        .env("SWAYWARD_I3_TEST", test)
        .env("SWAYWARD_I3_SKIPS", skip_adaptations.to_string())
        .env("SWAYWARD_I3_ADAPT", adaptations.to_string())
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
pub(super) fn i3_child_polling_yields_cpu_between_checks() {
    let started = Instant::now();
    for _ in 0..10 {
        pause_i3_poll();
    }
    assert!(started.elapsed() >= Duration::from_millis(5));
}

#[test]
fn i3_conformance_runner() {
    // `SWAYWARD_I3_TEST` selects a single file, including one with known
    // failures, so conformance findings stay executable without turning the
    // default gate red.
    if let Ok(selected) = std::env::var("SWAYWARD_I3_TEST") {
        run_i3_test_with_context(&selected, false);
        return;
    }

    let tests = passing_tests().collect::<Vec<_>>();
    assert!(
        !tests.is_empty(),
        "no fully green files derive from tests/i3/coverage.toml"
    );
    let failures = collect_test_failures(
        tests.iter().map(|test| (*test, true)),
        run_i3_test_with_context,
    );
    assert!(
        failures.is_empty(),
        "{} of {} green i3 files failed:\n{}",
        failures.len(),
        tests.len(),
        failures
            .iter()
            .map(|(test, message)| format!("{test}: {message}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}
