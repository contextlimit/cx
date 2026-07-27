use std::env;
use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};

use tempfile::tempdir;

fn path_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_test_environment() -> MutexGuard<'static, ()> {
    let lock = path_lock();
    match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            lock.clear_poison();
            poisoned.into_inner()
        }
    }
}

struct EnvironmentRestoreGuard {
    original: Vec<(String, Option<OsString>)>,
}

impl EnvironmentRestoreGuard {
    fn capture<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            original: names
                .into_iter()
                .map(|name| (name.to_string(), env::var_os(name)))
                .collect(),
        }
    }
}

impl Drop for EnvironmentRestoreGuard {
    fn drop(&mut self) {
        for (name, value) in self.original.drain(..) {
            match value {
                Some(value) => env::set_var(name, value),
                None => env::remove_var(name),
            }
        }
    }
}

pub fn with_fake_path<T>(entries: &[(&str, &str)], action: impl FnOnce() -> T) -> T {
    with_fake_path_inner(entries, false, action)
}

pub fn with_fake_path_only<T>(entries: &[(&str, &str)], action: impl FnOnce() -> T) -> T {
    with_fake_path_inner(entries, true, action)
}

pub fn with_env_vars<T>(entries: &[(&str, Option<&str>)], action: impl FnOnce() -> T) -> T {
    let _lock = lock_test_environment();
    let _restore = EnvironmentRestoreGuard::capture(entries.iter().map(|(name, _)| *name));

    for (name, value) in entries {
        match value {
            Some(value) => env::set_var(name, value),
            None => env::remove_var(name),
        }
    }

    action()
}

fn with_fake_path_inner<T>(
    entries: &[(&str, &str)],
    isolate: bool,
    action: impl FnOnce() -> T,
) -> T {
    let _lock = lock_test_environment();
    let dir = tempdir().expect("tempdir");
    for (name, body) in entries {
        write_executable(dir.path(), name, body);
    }

    let _restore = EnvironmentRestoreGuard::capture([
        "PATH",
        "CX_DISABLE_TOOL_FALLBACK_PATHS",
        "CX_TOOL_FALLBACK_PATHS",
    ]);
    let mut new_path = OsString::from(dir.path().as_os_str());
    if !isolate {
        if let Some(existing) = env::var_os("PATH") {
            new_path.push(if cfg!(windows) { ";" } else { ":" });
            new_path.push(existing);
        }
    }
    env::set_var("PATH", &new_path);
    env::set_var("CX_DISABLE_TOOL_FALLBACK_PATHS", "1");
    env::remove_var("CX_TOOL_FALLBACK_PATHS");

    action()
}

pub fn write_executable(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, body).expect("write executable");
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("set permissions");
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use super::*;

    #[test]
    fn panics_restore_environment_and_do_not_break_followup_scopes() {
        const KEY: &str = "CX_TEST_SUPPORT_PANIC_RESTORE";
        let original_key = with_env_vars(&[], || env::var_os(KEY));
        let original_path_state = with_env_vars(&[], || {
            (
                env::var_os("PATH"),
                env::var_os("CX_DISABLE_TOOL_FALLBACK_PATHS"),
                env::var_os("CX_TOOL_FALLBACK_PATHS"),
            )
        });

        let env_panic = catch_unwind(AssertUnwindSafe(|| {
            with_env_vars(&[(KEY, Some("temporary"))], || {
                assert_eq!(env::var(KEY).as_deref(), Ok("temporary"));
                panic!("intentional environment-scope panic");
            });
        }));
        assert!(env_panic.is_err());
        with_env_vars(&[], || assert_eq!(env::var_os(KEY), original_key));

        let path_panic = catch_unwind(AssertUnwindSafe(|| {
            with_fake_path(&[("cx-fixture", "#!/bin/sh\nexit 0\n")], || {
                assert_ne!(env::var_os("PATH"), original_path_state.0);
                panic!("intentional fake-path panic");
            });
        }));
        assert!(path_panic.is_err());
        with_env_vars(&[], || {
            assert_eq!(env::var_os("PATH"), original_path_state.0);
            assert_eq!(
                env::var_os("CX_DISABLE_TOOL_FALLBACK_PATHS"),
                original_path_state.1
            );
            assert_eq!(env::var_os("CX_TOOL_FALLBACK_PATHS"), original_path_state.2);
        });

        with_env_vars(&[(KEY, Some("followup"))], || {
            assert_eq!(env::var(KEY).as_deref(), Ok("followup"));
        });
        with_env_vars(&[], || assert_eq!(env::var_os(KEY), original_key));
    }
}
