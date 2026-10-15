//! Desktop adapter classification/forwarding only; no assets or hardware.
use super::*;
use crate::adv::higurashi::save_game::WriteIoFailure;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "higurashi-desktop-storage-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
struct ArbitraryFailure;
impl Storage for ArbitraryFailure {
    fn trial_token(&self) -> bool {
        false
    }
    fn read(&self, _: Source) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    fn write(&self, _: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("untyped storage logic failure")).context("outer storage context")
    }
}
struct Unsupported;
impl Storage for Unsupported {
    fn trial_token(&self) -> bool {
        false
    }
    fn read(&self, _: Source) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    // Deliberately retain the trait's default unsupported-write error.
}

#[test]
fn higurashi_desktop_storage_marks_contextual_io_but_preserves_untyped_failures() {
    let temporary = Temporary::new();
    let blocked = temporary.0.join("directory");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("marker"), b"retained").unwrap();
    struct ContextualPaths(Paths);
    impl Storage for ContextualPaths {
        fn trial_token(&self) -> bool {
            self.0.trial_token()
        }
        fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
            self.0.read(source)
        }
        fn trial_exists(&self) -> Result<bool> {
            self.0.trial_exists()
        }
        fn write(&self, bytes: &[u8]) -> Result<()> {
            self.0.write(bytes).context("outer contextual atomic IO")
        }
    }
    let storage = DesktopStorage(Arc::new(ContextualPaths(Paths {
        current: Some(blocked.clone()),
        trial: None,
    })));
    let error = storage.write(b"new progress").unwrap_err();
    assert!(error.is::<WriteIoFailure>());
    assert!(error.chain().any(|cause| cause.is::<std::io::Error>()));
    assert!(format!("{error:#}").contains("outer contextual atomic IO"));
    assert_eq!(std::fs::read(blocked.join("marker")).unwrap(), b"retained");
    assert_eq!(
        std::fs::read_dir(&temporary.0).unwrap().count(),
        1,
        "failed atomic temporary file cleaned"
    );

    for (storage, message) in [
        (
            Arc::new(ArbitraryFailure) as Arc<dyn Storage>,
            "untyped storage logic failure",
        ),
        (
            Arc::new(Unsupported) as Arc<dyn Storage>,
            "does not support writes",
        ),
        (
            Arc::new(Paths {
                current: None,
                trial: None,
            }) as Arc<dyn Storage>,
            "no writable path",
        ),
    ] {
        assert!(
            !storage.recover_write_errors(),
            "ordinary component storage keeps default policy"
        );
        let desktop = DesktopStorage(storage);
        assert!(desktop.recover_write_errors());
        let error = desktop.write(b"unchanged snapshot").unwrap_err();
        assert!(
            !error.is::<WriteIoFailure>(),
            "non-IO error was misclassified: {error:#}"
        );
        assert!(!error.chain().any(|cause| cause.is::<std::io::Error>()));
        assert!(format!("{error:#}").contains(message));
    }
}

#[test]
fn higurashi_desktop_storage_forwards_read_trial_and_successful_write_boundaries() {
    struct Forward {
        calls: Mutex<Vec<String>>,
        read_error: AtomicBool,
        path: PathBuf,
    }
    impl Storage for Forward {
        fn trial_token(&self) -> bool {
            self.calls.lock().unwrap().push("token".into());
            true
        }
        fn read(&self, source: Source) -> Result<Option<Vec<u8>>> {
            self.calls.lock().unwrap().push(format!("read:{source:?}"));
            if self.read_error.load(Ordering::SeqCst) {
                return std::fs::read(self.path.join("absent"))
                    .map(Some)
                    .context("read IO stays read IO");
            }
            Ok(if source == Source::Current {
                Some(vec![0x10, 0x20])
            } else {
                None
            })
        }
        fn trial_exists(&self) -> Result<bool> {
            self.calls.lock().unwrap().push("exists".into());
            Ok(true)
        }
        fn write(&self, bytes: &[u8]) -> Result<()> {
            self.calls.lock().unwrap().push("write".into());
            crate::adv::persistence::write_atomic(&self.path.join("progress.dat"), bytes)
        }
    }
    let temporary = Temporary::new();
    let inner = Arc::new(Forward {
        calls: Mutex::new(vec![]),
        read_error: AtomicBool::new(false),
        path: temporary.0.clone(),
    });
    assert!(!inner.recover_write_errors());
    let desktop = DesktopStorage(inner.clone());
    assert!(desktop.recover_write_errors());
    assert!(desktop.trial_token());
    assert_eq!(
        desktop.read(Source::Current).unwrap(),
        Some(vec![0x10, 0x20])
    );
    assert_eq!(desktop.read(Source::Trial).unwrap(), None);
    assert!(desktop.trial_exists().unwrap());
    desktop.write(b"exact frozen bytes").unwrap();
    assert_eq!(
        std::fs::read(temporary.0.join("progress.dat")).unwrap(),
        b"exact frozen bytes"
    );
    assert_eq!(
        *inner.calls.lock().unwrap(),
        ["token", "read:Current", "read:Trial", "exists", "write"]
    );
    inner.read_error.store(true, Ordering::SeqCst);
    let error = desktop.read(Source::Current).unwrap_err();
    assert!(
        !error.is::<WriteIoFailure>(),
        "read errors cannot opt a writer into recovery"
    );
    assert!(error.chain().any(|cause| cause.is::<std::io::Error>()));
    assert_eq!(
        std::fs::read(temporary.0.join("progress.dat")).unwrap(),
        b"exact frozen bytes"
    );
}
