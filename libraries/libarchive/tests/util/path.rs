use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

pub fn fixtures() -> PathBuf {
    root().join("fixtures")
}

pub fn fixture(name: &str) -> PathBuf {
    fixtures().join(name)
}

/// A fresh, empty directory below the system temporary directory that is
/// removed again when the value is dropped.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// `name` must be unique per test so that tests can run in parallel.
    pub fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("libarchive-tests-{}-{}", process::id(), name));
        match fs::remove_dir_all(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => panic!("remove stale temporary directory {}: {e}", path.display()),
        }
        // `create_dir` fails if the path exists (also as a symlink), so the
        // directory is guaranteed to be new and owned by this test.
        fs::create_dir(&path).expect("create temporary directory");
        TempDir { path }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn as_str(&self) -> &str {
        self.path.to_str().expect("temporary path is valid UTF-8")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
