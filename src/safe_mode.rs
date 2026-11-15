//! Safe mode: after an unclean exit (crash, SIGKILL, power loss) the daemon
//! does not restore the last wallpaper, so a wallpaper that takes the daemon
//! or the machine down cannot cause a crash loop. Restoring stays off until
//! the user applies a wallpaper themselves.
//!
//! A `running` marker is written at startup and removed on a clean shutdown;
//! finding it at startup means the previous run did not finish cleanly.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

const RUNNING: &str = "running";
const BLOCKED: &str = "restore-blocked";

static RESTORE_BLOCKED: AtomicBool = AtomicBool::new(false);
static STATE_DIR: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// The previous run left its marker behind.
    pub unclean_exit: bool,
    /// Restoring the last wallpaper is off for this run.
    pub restore_blocked: bool,
}

/// `$XDG_STATE_HOME/waywallen`, else `~/.local/state/waywallen`.
pub fn state_dir() -> Option<PathBuf> {
    crate::logging::paths::log_dir().and_then(|logs| logs.parent().map(Path::to_path_buf))
}

/// Record a startup in `dir`. With `enabled` false the marker is still kept
/// but nothing is ever blocked.
pub fn begin_in(dir: &Path, enabled: bool) -> std::io::Result<Outcome> {
    std::fs::create_dir_all(dir)?;
    let marker = dir.join(RUNNING);
    let unclean_exit = marker.exists();
    std::fs::write(&marker, std::process::id().to_string())?;
    if enabled && unclean_exit {
        std::fs::write(dir.join(BLOCKED), b"")?;
    }
    let restore_blocked = enabled && dir.join(BLOCKED).exists();
    Ok(Outcome {
        unclean_exit,
        restore_blocked,
    })
}

/// Mark a clean shutdown.
pub fn end_in(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(RUNNING));
}

/// The user chose a wallpaper: restoring works again from the next start.
pub fn unblock_in(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(BLOCKED));
}

pub fn begin(enabled: bool) -> Outcome {
    let Some(dir) = state_dir() else {
        return Outcome {
            unclean_exit: false,
            restore_blocked: false,
        };
    };
    let _ = STATE_DIR.set(dir.clone());
    let outcome = begin_in(&dir, enabled).unwrap_or_else(|e| {
        log::warn!("safe mode: cannot use {}: {e}", dir.display());
        Outcome {
            unclean_exit: false,
            restore_blocked: false,
        }
    });
    RESTORE_BLOCKED.store(outcome.restore_blocked, Ordering::SeqCst);
    outcome
}

pub fn end() {
    if let Some(dir) = STATE_DIR.get() {
        end_in(dir);
    }
}

/// Called when the user applies a wallpaper.
pub fn user_chose_wallpaper() {
    if RESTORE_BLOCKED.swap(false, Ordering::SeqCst) {
        log::info!("safe mode: wallpaper chosen, restoring is on again");
    }
    if let Some(dir) = STATE_DIR.get() {
        unblock_in(dir);
    }
}

pub fn restore_blocked() -> bool {
    RESTORE_BLOCKED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_start_restores() {
        let d = tempfile::tempdir().unwrap();
        let o = begin_in(d.path(), true).unwrap();
        assert_eq!(o, Outcome { unclean_exit: false, restore_blocked: false });
    }

    #[test]
    fn clean_shutdown_restores_next_time() {
        let d = tempfile::tempdir().unwrap();
        begin_in(d.path(), true).unwrap();
        end_in(d.path());
        let o = begin_in(d.path(), true).unwrap();
        assert!(!o.unclean_exit && !o.restore_blocked);
    }

    #[test]
    fn crash_blocks_restore_until_the_user_picks_a_wallpaper() {
        let d = tempfile::tempdir().unwrap();
        begin_in(d.path(), true).unwrap();
        // crash: no end_in
        let o = begin_in(d.path(), true).unwrap();
        assert!(o.unclean_exit && o.restore_blocked);
        // a clean run that picked nothing stays blocked
        end_in(d.path());
        assert!(begin_in(d.path(), true).unwrap().restore_blocked);
        // a repeated crash loop stays blocked too
        assert!(begin_in(d.path(), true).unwrap().restore_blocked);
        unblock_in(d.path());
        end_in(d.path());
        assert!(!begin_in(d.path(), true).unwrap().restore_blocked);
    }

    #[test]
    fn disabled_never_blocks() {
        let d = tempfile::tempdir().unwrap();
        begin_in(d.path(), false).unwrap();
        let o = begin_in(d.path(), false).unwrap();
        assert!(o.unclean_exit && !o.restore_blocked);
        assert!(!d.path().join(BLOCKED).exists());
    }
}
