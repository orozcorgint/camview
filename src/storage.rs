use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A segment is rotated when it reaches this size, even before the time limit.
pub const SEGMENT_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Never delete a file modified this recently: mpv may still be finalizing it.
pub const RECENT_GRACE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
struct Clip {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub deleted_files: u64,
    pub deleted_bytes: u64,
    pub remaining_bytes: u64,
}

fn is_date_dir(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

fn is_recording_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".mkv") else {
        return false;
    };
    let bytes = stem.as_bytes();
    bytes.len() >= 8
        && bytes[2] == b'-'
        && bytes[5] == b'-'
        && bytes[0..2].iter().all(u8::is_ascii_digit)
        && bytes[3..5].iter().all(u8::is_ascii_digit)
        && bytes[6..8].iter().all(u8::is_ascii_digit)
}

/// Scans the CamView recording layout: `<root>/<camera>/<YYYY-MM-DD>/<HH-MM-SS...>.mkv`.
/// It never follows symlinks and ignores files that do not match the layout.
fn scan(root: &Path) -> Vec<Clip> {
    let mut clips = Vec::new();
    let Ok(cameras) = fs::read_dir(root) else {
        return clips;
    };

    for camera in cameras.flatten() {
        let Ok(camera_type) = camera.file_type() else {
            continue;
        };
        if !camera_type.is_dir() {
            continue;
        }
        let Ok(dates) = fs::read_dir(camera.path()) else {
            continue;
        };
        for date in dates.flatten() {
            let Ok(date_type) = date.file_type() else {
                continue;
            };
            if !date_type.is_dir() {
                continue;
            }
            let date_name = date.file_name();
            let Some(date_name) = date_name.to_str() else {
                continue;
            };
            if !is_date_dir(date_name) {
                continue;
            }
            let Ok(files) = fs::read_dir(date.path()) else {
                continue;
            };
            for file in files.flatten() {
                let Ok(file_type) = file.file_type() else {
                    continue;
                };
                if !file_type.is_file() {
                    continue;
                }
                let name = file.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                if !is_recording_name(name) {
                    continue;
                }
                let Ok(meta) = file.metadata() else {
                    continue;
                };
                clips.push(Clip {
                    path: file.path(),
                    size: meta.len(),
                    modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                });
            }
        }
    }

    clips
}

pub fn usage_bytes(root: &Path) -> u64 {
    scan(root).iter().map(|clip| clip.size).sum()
}

/// Deletes the oldest finished segments until the folder is at or below `max_bytes`.
/// Active recordings and files younger than [`RECENT_GRACE`] are never removed.
pub fn prune(
    root: &Path,
    max_bytes: u64,
    active: &HashSet<PathBuf>,
) -> Result<PruneReport, String> {
    prune_with_min_age(root, max_bytes, active, RECENT_GRACE)
}

fn prune_with_min_age(
    root: &Path,
    max_bytes: u64,
    active: &HashSet<PathBuf>,
    min_age: Duration,
) -> Result<PruneReport, String> {
    let mut clips = scan(root);
    let total: u64 = clips.iter().map(|clip| clip.size).sum();
    let mut report = PruneReport {
        remaining_bytes: total,
        ..PruneReport::default()
    };
    if max_bytes == 0 || total <= max_bytes {
        return Ok(report);
    }

    clips.sort_by(|a, b| {
        a.modified
            .cmp(&b.modified)
            .then_with(|| a.path.cmp(&b.path))
    });

    let now = SystemTime::now();
    for clip in clips {
        if report.remaining_bytes <= max_bytes {
            break;
        }
        if active.contains(&clip.path) {
            continue;
        }
        if now.duration_since(clip.modified).unwrap_or(Duration::ZERO) < min_age {
            continue;
        }
        match fs::remove_file(&clip.path) {
            Ok(()) => {
                report.deleted_files += 1;
                report.deleted_bytes = report.deleted_bytes.saturating_add(clip.size);
                report.remaining_bytes = report.remaining_bytes.saturating_sub(clip.size);
            }
            Err(e) => {
                crate::platform::log_line(&format!(
                    "Could not remove old recording {}: {e}",
                    clip.path.display()
                ));
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::unix::fs::symlink;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "camview-storage-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("cam/2026-01-01")).expect("create test dirs");
        root
    }

    fn clip(root: &Path, name: &str, size: usize, age_secs: u64) -> PathBuf {
        let path = root.join("cam/2026-01-01").join(name);
        fs::write(&path, vec![0u8; size]).expect("write clip");
        let modified = SystemTime::now() - Duration::from_secs(age_secs);
        let times = fs::FileTimes::new().set_modified(modified);
        OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open clip")
            .set_times(times)
            .expect("set mtime");
        path
    }

    #[test]
    fn prune_deletes_oldest_finished_clips() {
        let root = temp_root("oldest");
        let oldest = clip(&root, "10-00-00.mkv", 100, 300);
        let middle = clip(&root, "11-00-00.mkv", 100, 200);
        let newest = clip(&root, "12-00-00.mkv", 100, 100);

        let report =
            prune_with_min_age(&root, 150, &HashSet::new(), Duration::ZERO).expect("prune");

        assert!(!oldest.exists());
        assert!(!middle.exists());
        assert!(newest.exists());
        assert_eq!(report.deleted_files, 2);
        assert_eq!(report.deleted_bytes, 200);
        assert_eq!(report.remaining_bytes, 100);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prune_never_deletes_active_or_recent_clips() {
        let root = temp_root("active");
        let active = clip(&root, "10-00-00.mkv", 100, 300);
        let recent = clip(&root, "11-00-00.mkv", 100, 0);
        let old = clip(&root, "12-00-00.mkv", 100, 200);
        let mut active_set = HashSet::new();
        active_set.insert(active.clone());

        let report = prune_with_min_age(&root, 50, &active_set, RECENT_GRACE).expect("prune");

        assert!(active.exists());
        assert!(recent.exists());
        assert!(!old.exists());
        assert_eq!(report.deleted_files, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prune_ignores_other_files_and_symlinks() {
        let root = temp_root("other");
        let cam_dir = root.join("cam/2026-01-01");
        let old = clip(&root, "10-00-00.mkv", 100, 200);
        fs::write(cam_dir.join("notes.txt"), b"keep me").expect("write notes");
        let outside = root.join("outside.mkv");
        fs::write(&outside, vec![0u8; 100]).expect("write outside");
        symlink(&outside, cam_dir.join("11-00-00.mkv")).expect("symlink");

        let report = prune_with_min_age(&root, 1, &HashSet::new(), Duration::ZERO).expect("prune");

        assert!(!old.exists());
        assert!(cam_dir.join("notes.txt").exists());
        assert!(outside.exists());
        assert_eq!(report.deleted_files, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recognizes_old_and_new_recording_names() {
        assert!(is_recording_name("10-00-00.mkv"));
        assert!(is_recording_name("10-00-00-123.mkv"));
        assert!(is_recording_name("10-00-00-123-2.mkv"));
        assert!(!is_recording_name("notes.mkv"));
        assert!(!is_recording_name("10-00-00.mp4"));
        assert!(is_date_dir("2026-11-16"));
        assert!(!is_date_dir("not-a-date"));
        assert!(!is_date_dir("2026-1-16"));
    }

    #[test]
    fn usage_only_counts_recording_layout() {
        let root = temp_root("usage");
        clip(&root, "10-00-00.mkv", 100, 10);
        clip(&root, "11-00-00.mkv", 200, 10);
        fs::write(root.join("cam/2026-01-01/notes.txt"), b"ignored").expect("write notes");
        assert_eq!(usage_bytes(&root), 300);
        let _ = fs::remove_dir_all(root);
    }
}
