use std::fs;
use std::path::Path;

use crate::ConfigPath;

#[test]
fn resolved_reports_the_path_that_load_reads() {
    let dir = tempfile::tempdir_in(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target"),
    )
    .unwrap();
    let user = dir.path().join("user.kdl");
    let system = dir.path().join("system.kdl");

    // Explicit always wins, even when the file is absent.
    let missing = dir.path().join("nope.kdl");
    assert_eq!(
        ConfigPath::Explicit(missing.clone()).resolved(),
        Some(missing.as_path())
    );

    let regular = ConfigPath::Regular {
        user_path: user.clone(),
        system_path: system.clone(),
    };
    // Nothing on disk: load_or_create decides, so there is no path yet.
    assert_eq!(regular.resolved(), None);

    fs::write(&system, "").unwrap();
    assert_eq!(regular.resolved(), Some(system.as_path()));

    // The user path takes priority once it exists.
    fs::write(&user, "").unwrap();
    assert_eq!(regular.resolved(), Some(user.as_path()));
}
