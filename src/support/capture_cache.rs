use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use fs2::FileExt;

const LOCK_SUFFIX: &str = ".capture.lock";
const PENDING_SUFFIX: &str = ".capture.pending";
const STDIN_SUFFIX: &str = ".stdin";
const STDOUT_SUFFIX: &str = ".stdout";
const STDERR_SUFFIX: &str = ".stderr";
const ABANDONED_PENDING_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) struct CaptureLease {
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    lock_path: PathBuf,
    lock_file: Option<File>,
}

impl CaptureLease {
    pub(crate) fn create(root: &Path, unique: &str) -> Result<(Self, File, File)> {
        ensure_capture_root(root)?;
        reclaim_abandoned(root);

        let stdout_path = root.join(format!("{unique}{STDOUT_SUFFIX}"));
        let stderr_path = root.join(format!("{unique}{STDERR_SUFFIX}"));
        let (lock_path, lock_file) = create_lock(root, unique)?;

        let lease = Self {
            stdout_path,
            stderr_path,
            lock_path,
            lock_file: Some(lock_file),
        };
        let stdout = open_new(&lease.stdout_path)?;
        let stderr = open_new(&lease.stderr_path)?;
        Ok((lease, stdout, stderr))
    }

    pub(crate) fn stdout_path(&self) -> &Path {
        &self.stdout_path
    }

    pub(crate) fn stderr_path(&self) -> &Path {
        &self.stderr_path
    }
}

pub(crate) struct CaptureInputLease {
    stdin_path: PathBuf,
    lock_path: PathBuf,
    lock_file: Option<File>,
}

impl CaptureInputLease {
    pub(crate) fn create(root: &Path, unique: &str) -> Result<(Self, File)> {
        ensure_capture_root(root)?;
        reclaim_abandoned(root);

        let stdin_path = root.join(format!("{unique}{STDIN_SUFFIX}"));
        let (lock_path, lock_file) = create_lock(root, unique)?;
        let lease = Self {
            stdin_path,
            lock_path,
            lock_file: Some(lock_file),
        };
        let stdin = open_new(&lease.stdin_path)?;
        Ok((lease, stdin))
    }

    pub(crate) fn stdin_path(&self) -> &Path {
        &self.stdin_path
    }
}

impl Drop for CaptureInputLease {
    fn drop(&mut self) {
        if let Some(lock_file) = self.lock_file.take() {
            let _ = FileExt::unlock(&lock_file);
            drop(lock_file);
        }
        let _ = fs::remove_file(&self.stdin_path);
        let _ = fs::remove_file(&self.lock_path);
    }
}

impl Drop for CaptureLease {
    fn drop(&mut self) {
        if let Some(lock_file) = self.lock_file.take() {
            let _ = FileExt::unlock(&lock_file);
            drop(lock_file);
        }
        let _ = fs::remove_file(&self.stdout_path);
        let _ = fs::remove_file(&self.stderr_path);
        let _ = fs::remove_file(&self.lock_path);
    }
}

fn ensure_capture_root(root: &Path) -> Result<()> {
    if root.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(root).with_context(|| format!("failed to create {}", root.display()))
}

fn open_new(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("failed to create {}", path.display()))
}

fn create_lock(root: &Path, unique: &str) -> Result<(PathBuf, File)> {
    let pending_path = root.join(format!("{unique}{PENDING_SUFFIX}"));
    let lock_path = root.join(format!("{unique}{LOCK_SUFFIX}"));
    let lock_file = open_new(&pending_path)?;
    lock_file
        .lock_exclusive()
        .with_context(|| format!("failed to lock {}", pending_path.display()))?;
    fs::rename(&pending_path, &lock_path).with_context(|| {
        format!(
            "failed to publish capture lock {} as {}",
            pending_path.display(),
            lock_path.display()
        )
    })?;
    Ok((lock_path, lock_file))
}

fn reclaim_abandoned(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some(unique) = name.strip_suffix(LOCK_SUFFIX) {
            reclaim_locked_group(root, unique, &path);
        } else if name.ends_with(PENDING_SUFFIX) && pending_is_abandoned(&path) {
            let _ = fs::remove_file(path);
        }
    }
}

fn reclaim_locked_group(root: &Path, unique: &str, lock_path: &Path) {
    let Ok(lock_file) = OpenOptions::new().read(true).write(true).open(lock_path) else {
        return;
    };
    match lock_file.try_lock_exclusive() {
        Ok(()) => {
            let _ = fs::remove_file(root.join(format!("{unique}{STDIN_SUFFIX}")));
            let _ = fs::remove_file(root.join(format!("{unique}{STDOUT_SUFFIX}")));
            let _ = fs::remove_file(root.join(format!("{unique}{STDERR_SUFFIX}")));
            let _ = FileExt::unlock(&lock_file);
            drop(lock_file);
            let _ = fs::remove_file(lock_path);
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        Err(_) => {}
    }
}

fn pending_is_abandoned(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= ABANDONED_PENDING_AGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_removes_its_files_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let (lease, stdout, stderr) = CaptureLease::create(temp.path(), "tool-1-1").unwrap();
        drop(stdout);
        drop(stderr);
        assert!(lease.stdout_path().exists());
        assert!(lease.stderr_path().exists());
        assert!(lease.lock_path.exists());

        drop(lease);

        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[test]
    fn reclaim_removes_only_unlocked_capture_groups() {
        let temp = tempfile::tempdir().unwrap();
        let active_lock = create_group(temp.path(), "active");
        let abandoned_lock = create_group(temp.path(), "abandoned");
        FileExt::unlock(&abandoned_lock).unwrap();
        drop(abandoned_lock);

        reclaim_abandoned(temp.path());

        assert!(temp.path().join(format!("active{LOCK_SUFFIX}")).exists());
        assert!(temp.path().join(format!("active{STDOUT_SUFFIX}")).exists());
        assert!(!temp.path().join(format!("abandoned{LOCK_SUFFIX}")).exists());
        assert!(!temp
            .path()
            .join(format!("abandoned{STDOUT_SUFFIX}"))
            .exists());

        FileExt::unlock(&active_lock).unwrap();
    }

    #[test]
    fn reclaim_ignores_legacy_unowned_capture_files() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join("rg-123-456.stdout");
        fs::write(&legacy, "evidence").unwrap();

        reclaim_abandoned(temp.path());

        assert_eq!(fs::read_to_string(legacy).unwrap(), "evidence");
    }

    #[test]
    fn input_lease_removes_its_spool_and_lock() {
        let temp = tempfile::tempdir().unwrap();
        let (lease, stdin) = CaptureInputLease::create(temp.path(), "stdin-1").unwrap();
        drop(stdin);
        assert!(lease.stdin_path().exists());
        assert!(lease.lock_path.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(lease.stdin_path())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        drop(lease);

        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    fn create_group(root: &Path, unique: &str) -> File {
        fs::write(root.join(format!("{unique}{STDOUT_SUFFIX}")), "stdout").unwrap();
        fs::write(root.join(format!("{unique}{STDERR_SUFFIX}")), "stderr").unwrap();
        let lock = open_new(&root.join(format!("{unique}{LOCK_SUFFIX}"))).unwrap();
        lock.lock_exclusive().unwrap();
        lock
    }
}
