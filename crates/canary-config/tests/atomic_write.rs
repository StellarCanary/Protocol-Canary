//! Coverage for the atomic lockfile writer (CF-03 section 6).
//!
//! Every test works through the crate's public API. The writer must never
//! expose a partially written `.stellar-canary.lock`: concurrent writers fail
//! with `LockError::Busy`, an interrupted write leaves the previous lock
//! byte-for-byte intact, and a successful write leaves no temporary file.

use std::path::PathBuf;
use std::sync::{Arc, Barrier};

use canary_config::{
    load_lock, serialize_lock, write_lock, LockError, LockFile, LockFixtures, LOCK_FILE_NAME,
    SUPPORTED_LOCK_VERSION, SUPPORTED_REGISTRY_VERSION,
};

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn lock(protocol: u32) -> LockFile {
    LockFile {
        lock_version: SUPPORTED_LOCK_VERSION,
        tool: None,
        fixtures: LockFixtures {
            protocol,
            pack_digest: DIGEST.to_string(),
            registry_version: SUPPORTED_REGISTRY_VERSION,
            source: None,
            revision: None,
        },
    }
}

/// A scratch directory removed on drop, with no dependency beyond `std`.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "canary-atomic-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }

    fn lock_path(&self) -> PathBuf {
        self.0.join(LOCK_FILE_NAME)
    }

    fn temp_path(&self) -> PathBuf {
        self.0.join(format!("{LOCK_FILE_NAME}.tmp"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn writes_a_complete_lock_and_leaves_no_temporary_file() {
    let dir = Scratch::new("write");
    let expected = lock(28);

    write_lock(&dir.lock_path(), &expected).expect("write");
    assert_eq!(load_lock(&dir.lock_path()).unwrap(), expected);
    assert!(
        !dir.temp_path().exists(),
        "the temporary file must be renamed away"
    );
}

#[test]
fn atomically_replaces_an_existing_lock() {
    let dir = Scratch::new("replace");
    write_lock(&dir.lock_path(), &lock(28)).unwrap();

    write_lock(&dir.lock_path(), &lock(29)).expect("overwrite");
    assert_eq!(load_lock(&dir.lock_path()).unwrap().fixtures.protocol, 29);
    assert!(!dir.temp_path().exists());

    // The replaced bytes are exactly the canonical serialization, not a
    // prefix or concatenation of the two writes.
    assert_eq!(
        std::fs::read_to_string(dir.lock_path()).unwrap(),
        serialize_lock(&lock(29)).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn writes_the_lock_with_mode_0644() {
    use std::os::unix::fs::PermissionsExt;

    let dir = Scratch::new("permissions");
    write_lock(&dir.lock_path(), &lock(28)).unwrap();

    let mode = std::fs::metadata(dir.lock_path())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o644, "mode was {:o}", mode & 0o777);
}

#[test]
fn an_in_progress_writer_makes_a_second_writer_fail_without_touching_the_lock() {
    let dir = Scratch::new("busy");
    let original = lock(28);
    write_lock(&dir.lock_path(), &original).unwrap();
    let before = std::fs::read(dir.lock_path()).unwrap();

    // Another process has claimed the fixed temporary name but not finished.
    std::fs::write(dir.temp_path(), b"partial lock").unwrap();

    let error = write_lock(&dir.lock_path(), &lock(29)).unwrap_err();
    assert!(matches!(error, LockError::Busy { .. }), "{error}");
    assert_eq!(
        std::fs::read(dir.lock_path()).unwrap(),
        before,
        "an interrupted write must leave the original lock untouched"
    );
    assert_eq!(
        std::fs::read(dir.temp_path()).unwrap(),
        b"partial lock",
        "a failed writer must not delete another writer's temporary file"
    );
}

#[test]
fn a_lock_that_fails_validation_never_touches_the_filesystem() {
    let dir = Scratch::new("invalid");
    let original = lock(28);
    write_lock(&dir.lock_path(), &original).unwrap();
    let before = std::fs::read(dir.lock_path()).unwrap();

    let mut invalid = lock(29);
    invalid.fixtures.pack_digest = "md5:abc".to_string();
    assert!(matches!(
        write_lock(&dir.lock_path(), &invalid),
        Err(LockError::Invalid { .. })
    ));
    assert_eq!(std::fs::read(dir.lock_path()).unwrap(), before);
    assert!(!dir.temp_path().exists());
}

#[cfg(unix)]
#[test]
fn refuses_to_write_through_a_symbolic_link() {
    let dir = Scratch::new("symlink");
    let outside = dir.0.join("outside.lock");
    std::fs::write(&outside, "do not replace me").unwrap();
    std::os::unix::fs::symlink(&outside, dir.lock_path()).unwrap();

    let error = write_lock(&dir.lock_path(), &lock(28)).unwrap_err();
    assert!(matches!(error, LockError::Symlink { .. }), "{error}");
    assert_eq!(
        std::fs::read_to_string(&outside).unwrap(),
        "do not replace me"
    );
    assert!(!dir.temp_path().exists());
}

#[test]
fn concurrent_writers_never_produce_a_partial_lock() {
    const WRITERS: usize = 8;
    const ROUNDS: usize = 20;

    let dir = Scratch::new("concurrent");
    let path = Arc::new(dir.lock_path());

    for round in 0..ROUNDS {
        let barrier = Arc::new(Barrier::new(WRITERS));
        let mut handles = Vec::with_capacity(WRITERS);
        for writer in 0..WRITERS {
            let path = Arc::clone(&path);
            let barrier = Arc::clone(&barrier);
            let candidate = lock(u32::try_from(writer).unwrap() + 1);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                (write_lock(&path, &candidate), candidate)
            }));
        }

        let mut succeeded = Vec::new();
        for handle in handles {
            match handle.join().unwrap() {
                (Ok(()), candidate) => succeeded.push(candidate),
                (Err(LockError::Busy { .. }), _) => {}
                (Err(error), _) => panic!("round {round}: unexpected write error: {error}"),
            }
        }
        assert!(
            !succeeded.is_empty(),
            "round {round}: at least one concurrent writer must win"
        );

        // Whatever order the writers finished in, the visible file is one
        // complete canonical lock, and never another writer's torn bytes.
        let bytes = std::fs::read_to_string(&*path).unwrap();
        let parsed = load_lock(&path).expect("a concurrent write must leave a loadable lock");
        assert!(
            succeeded.contains(&parsed),
            "round {round}: unexpected lock"
        );
        assert_eq!(bytes, serialize_lock(&parsed).unwrap());
        assert!(
            !dir.temp_path().exists(),
            "round {round}: leftover temporary file"
        );
    }
}
