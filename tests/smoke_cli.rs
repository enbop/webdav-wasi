#![cfg(not(target_family = "wasm"))]

use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "webdav-smoke-cli-{timestamp}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

enum Root {
    Auto,
    Environment,
    Explicit,
}

fn check_smoke(root: Root, populated: bool) {
    let temp = TestDir::new();
    let path = temp.0.join(match root {
        Root::Auto => "data",
        _ => "configured-root",
    });
    let sentinel = b"Existing user content must survive smoke testing, without truncation.\n";
    if populated {
        fs::create_dir(&path).unwrap();
        fs::write(path.join("hello.txt"), sentinel).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_webdav-wasi"));
    command
        .current_dir(&temp.0)
        .env_remove("WEBDAV_FS_ROOT")
        .arg("--smoke-test");
    match root {
        Root::Auto => {}
        Root::Environment => {
            command.env("WEBDAV_FS_ROOT", &path);
        }
        Root::Explicit => {
            command.arg("--fs-root").arg(&path);
        }
    }
    let result = command.output().expect("run smoke test");
    assert!(
        result.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("smoke test passed"));
    if populated {
        assert_eq!(fs::read(path.join("hello.txt")).unwrap(), sentinel);
        assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
    } else {
        assert!(
            !path.exists(),
            "smoke test must not initialize a service directory"
        );
    }
}

#[test]
fn smoke_preserves_auto_detected_data() {
    check_smoke(Root::Auto, true);
}

#[test]
fn smoke_preserves_environment_root() {
    check_smoke(Root::Environment, true);
}

#[test]
fn smoke_preserves_explicit_root() {
    check_smoke(Root::Explicit, true);
}

#[test]
fn smoke_does_not_create_configured_root() {
    check_smoke(Root::Explicit, false);
}
