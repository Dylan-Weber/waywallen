//! Safe mode: after an unclean exit (crash, SIGKILL, power loss) the daemon
//! does not restore the last wallpaper, so a wallpaper that takes the daemon
//! or the machine down cannot cause a crash loop. Restoring stays off until
//! the user applies a wallpaper themselves.
//!
//! A `running` marker is written at startup and removed on a clean shutdown;
//! finding it at startup means the previous run did not finish cleanly.
//! The marker is also removed once a run has stayed up for [`SETTLE_AFTER`]
//! (and written again when a wallpaper is chosen), so only an exit while a
//! wallpaper is still starting counts: a later crash, a power cut or a
//! suspend that never resumes is not a crash loop and must not cost the user
//! their wallpaper at the next login.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

const RUNNING: &str = "running";
const BLOCKED: &str = "restore-blocked";

/// A run that stays up this long was not taken down by loading its wallpaper.
pub const SETTLE_AFTER: Duration = Duration::from_secs(120);

static RESTORE_BLOCKED: AtomicBool = AtomicBool::new(false);
static STATE_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Bumped by every arm, so only the latest arm's timer settles.
static ARM_GENERATION: AtomicU64 = AtomicU64::new(0);

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

/// The run outlived the crash-loop window: how it ends no longer matters.
pub fn settle_in(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(RUNNING));
}

/// A wallpaper is starting: an exit before it settles counts as unclean again.
pub fn arm_in(dir: &Path) -> std::io::Result<()> {
    std::fs::write(dir.join(RUNNING), std::process::id().to_string())
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
    settle_later(dir);
    outcome
}

fn settle_later(dir: PathBuf) {
    let generation = ARM_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let spawned = std::thread::Builder::new()
        .name("safe-mode-settle".into())
        .spawn(move || {
            std::thread::sleep(SETTLE_AFTER);
            if ARM_GENERATION.load(Ordering::SeqCst) == generation {
                settle_in(&dir);
            }
        });
    if let Err(e) = spawned {
        log::warn!("safe mode: cannot start the settle timer: {e}");
    }
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
        if let Err(e) = arm_in(dir) {
            log::warn!("safe mode: cannot use {}: {e}", dir.display());
        }
        settle_later(dir.clone());
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
    fn an_exit_after_settling_does_not_block() {
        let d = tempfile::tempdir().unwrap();
        begin_in(d.path(), true).unwrap();
        settle_in(d.path());
        // power cut or a suspend that never resumed: no end_in
        let o = begin_in(d.path(), true).unwrap();
        assert_eq!(o, Outcome { unclean_exit: false, restore_blocked: false });
    }

    #[test]
    fn choosing_a_wallpaper_arms_the_check_again() {
        let d = tempfile::tempdir().unwrap();
        begin_in(d.path(), true).unwrap();
        settle_in(d.path());
        arm_in(d.path()).unwrap();
        // the chosen wallpaper takes the daemon down before settling
        let o = begin_in(d.path(), true).unwrap();
        assert!(o.unclean_exit && o.restore_blocked);
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
