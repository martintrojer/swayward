use std::fs;

use crate::ConfigPath;

#[test]
fn resolved_reports_the_path_that_load_reads() {
    let dir = std::env::temp_dir().join(format!("swayward-resolved-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let user = dir.join("user.kdl");
    let system = dir.join("system.kdl");
    let _ = fs::remove_file(&user);
    let _ = fs::remove_file(&system);

    // Explicit always wins, even when the file is absent.
    let missing = dir.join("nope.kdl");
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

    fs::remove_dir_all(&dir).unwrap();
}
