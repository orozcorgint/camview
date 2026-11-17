use std::ffi::{c_char, c_void, CString};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use chrono::Local;

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_dir() -> PathBuf {
    home().join(".config/camview")
}

pub fn state_dir() -> PathBuf {
    home().join(".local/state/camview")
}

pub fn default_storage_dir() -> PathBuf {
    home().join("Videos/CamView")
}

pub fn snapshots_dir() -> PathBuf {
    home().join("Pictures/CamView")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn ensure_dir(p: &Path) -> Result<(), String> {
    fs::create_dir_all(p).map_err(|e| format!("Could not create {}: {e}", p.display()))?;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o700));
    Ok(())
}

pub fn clock() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn date_dir() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub fn time_file_ms() -> String {
    Local::now().format("%H-%M-%S-%3f").to_string()
}

pub fn date_time_file() -> String {
    Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

static LOG_LOCK: Mutex<()> = Mutex::new(());

pub fn log_line(msg: &str) {
    let _guard = LOG_LOCK.lock().ok();
    eprintln!("[camview] {msg}");
    let dir = state_dir();
    let _ = ensure_dir(&dir);
    let path = dir.join("camview.log");
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > 1_000_000 {
            let _ = fs::rename(&path, dir.join("camview.log.1"));
        }
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", clock(), msg);
    }
}

/// Hides URL credentials before logging or displaying them.
pub fn redact_url(s: &str) -> String {
    if let Some(scheme) = s.find("://") {
        let start = scheme + 3;
        if let Some(rel) = s[start..].find('@') {
            let mut out = s.to_string();
            out.replace_range(start..start + rel, "***");
            return out;
        }
    }
    s.to_string()
}

pub fn sanitize_id(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    while out.starts_with('-') {
        out.remove(0);
    }
    if out.is_empty() {
        "camera".into()
    } else {
        out
    }
}

pub fn format_mb(mb: u64) -> String {
    if mb >= 1024 {
        format!("{:.1} GB", mb as f64 / 1024.0)
    } else {
        format!("{mb} MB")
    }
}

pub fn disk_free_mb(path: &Path) -> Option<u64> {
    let c = CString::new(path.to_string_lossy().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if rc == 0 {
        Some((st.f_bavail as u64 * st.f_frsize as u64) / (1024 * 1024))
    } else {
        None
    }
}

pub fn mem_available_mb() -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemAvailable:") {
            let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            return Some(kb / 1024);
        }
    }
    None
}

pub fn open_path(path: &Path) {
    let _ = Command::new("xdg-open").arg(path).spawn();
}

pub fn acquire_single_instance() -> Option<File> {
    let dir = home().join(".cache/camview");
    let _ = ensure_dir(&dir);
    let path = dir.join("camview.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .ok()?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        Some(file)
    } else {
        None
    }
}

pub fn autostart_path() -> PathBuf {
    home().join(".config/autostart/camview.desktop")
}

pub fn is_autostart_enabled() -> bool {
    autostart_path().exists()
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let path = autostart_path();
    if enabled {
        if let Some(dir) = path.parent() {
            ensure_dir(dir)?;
        }
        let exe = std::env::current_exe()
            .map_err(|e| format!("Could not determine the executable: {e}"))?;
        let body = format!(
            "[Desktop Entry]\nType=Application\nVersion=1.0\nName=CamView\nComment=Lightweight IP camera viewer and recorder\nExec=\"{}\"\nIcon=camview\nTerminal=false\nCategories=AudioVideo;Video;Security;\nX-GNOME-Autostart-enabled=true\nX-GNOME-Autostart-Delay=10\n",
            exe.display()
        );
        fs::write(&path, body).map_err(|e| format!("Could not write autostart: {e}"))?;
    } else if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("Could not remove autostart: {e}"))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// OpenGL function loading for libmpv.
// Uses the system EGL/GLX loaders instead of eframe's closure.
// ---------------------------------------------------------------------------

type ProcFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;

#[derive(Clone, Copy)]
struct GlFns {
    egl: Option<ProcFn>,
    glx: Option<ProcFn>,
    glx_arb: Option<ProcFn>,
}

unsafe impl Send for GlFns {}
unsafe impl Sync for GlFns {}

fn gl_fns() -> &'static GlFns {
    static FNS: OnceLock<GlFns> = OnceLock::new();
    FNS.get_or_init(|| unsafe {
        let egl = load_sym(b"libEGL.so.1\0", b"eglGetProcAddress\0").and_then(|p| to_proc(p));
        let glx = load_sym(b"libGL.so.1\0", b"glXGetProcAddress\0").and_then(|p| to_proc(p));
        let glx_arb = load_sym(b"libGL.so.1\0", b"glXGetProcAddressARB\0").and_then(|p| to_proc(p));
        GlFns { egl, glx, glx_arb }
    })
}

unsafe fn load_sym(lib: &[u8], sym: &[u8]) -> Option<*mut c_void> {
    let lib = lib.as_ptr() as *const c_char;
    let sym = sym.as_ptr() as *const c_char;
    let handle = libc::dlopen(lib, libc::RTLD_LAZY | libc::RTLD_LOCAL);
    if handle.is_null() {
        return None;
    }
    let p = libc::dlsym(handle, sym);
    if p.is_null() {
        None
    } else {
        Some(p)
    }
}

fn to_proc(p: *mut c_void) -> Option<ProcFn> {
    unsafe { std::mem::transmute::<*mut c_void, Option<ProcFn>>(p) }
}

pub fn gl_get_proc(name: &str) -> *mut c_void {
    let Ok(cname) = CString::new(name) else {
        return std::ptr::null_mut();
    };
    let fns = gl_fns();
    unsafe {
        if let Some(f) = fns.egl {
            let p = f(cname.as_ptr());
            if !p.is_null() {
                return p;
            }
        }
        if let Some(f) = fns.glx_arb {
            let p = f(cname.as_ptr());
            if !p.is_null() {
                return p;
            }
        }
        if let Some(f) = fns.glx {
            let p = f(cname.as_ptr());
            if !p.is_null() {
                return p;
            }
        }
        let handle = libc::dlopen(std::ptr::null(), libc::RTLD_LAZY);
        if !handle.is_null() {
            let p = libc::dlsym(handle, cname.as_ptr());
            if !p.is_null() {
                return p;
            }
        }
    }
    std::ptr::null_mut()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_ids_are_filesystem_friendly() {
        assert_eq!(sanitize_id("Front Door"), "front-door");
        assert_eq!(sanitize_id("  Patio / Norte  "), "patio-norte");
        assert_eq!(sanitize_id("***"), "camera");
    }

    #[test]
    fn redacts_url_credentials() {
        assert_eq!(
            redact_url("rtsp://viewer:secret@127.0.0.1:8554/stream0"),
            "rtsp://***@127.0.0.1:8554/stream0"
        );
        assert_eq!(
            redact_url("rtsp://127.0.0.1:8554/stream0"),
            "rtsp://127.0.0.1:8554/stream0"
        );
    }

    #[test]
    fn formats_units() {
        assert_eq!(format_mb(512), "512 MB");
        assert_eq!(format_mb(2048), "2.0 GB");
    }
}
