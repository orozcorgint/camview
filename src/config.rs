use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::platform;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    Sub,
    Main,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Camera {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub tls: bool,
    #[serde(default = "default_rtsp_port")]
    pub rtsp_port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_main_path")]
    pub main_path: String,
    #[serde(default = "default_sub_path")]
    pub sub_path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_rtsp_port() -> u16 {
    554
}
fn default_main_path() -> String {
    "/stream0".into()
}
fn default_sub_path() -> String {
    "/stream1".into()
}
fn default_true() -> bool {
    true
}

impl Camera {
    pub fn stream_path(&self, kind: StreamKind) -> &str {
        match kind {
            StreamKind::Sub => &self.sub_path,
            StreamKind::Main => &self.main_path,
        }
    }

    /// Local URL consumed by mpv. TLS cameras use the socat tunnel on 127.0.0.1.
    pub fn url_for(&self, local_port: Option<u16>, kind: StreamKind) -> String {
        let (host, port) = if self.tls {
            match local_port {
                Some(p) => ("127.0.0.1".to_string(), p),
                None => (self.host.clone(), self.rtsp_port),
            }
        } else {
            (self.host.clone(), self.rtsp_port)
        };
        let path = self.stream_path(kind);
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let user = utf8_percent_encode(&self.user, NON_ALPHANUMERIC).to_string();
        let pass = utf8_percent_encode(&self.password, NON_ALPHANUMERIC).to_string();
        if user.is_empty() {
            format!("rtsp://{host}:{port}{path}")
        } else {
            format!("rtsp://{user}:{pass}@{host}:{port}{path}")
        }
    }

    pub fn slug(&self) -> String {
        platform::sanitize_id(&self.id)
    }

    pub fn host_label(&self) -> String {
        if self.tls {
            format!("{}:{} (TLS)", self.host, self.rtsp_port)
        } else {
            format!("{}:{}", self.host, self.rtsp_port)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default = "default_fps")]
    pub render_fps: u32,
    #[serde(default = "default_max_tiles")]
    pub max_live_tiles: usize,
    #[serde(default)]
    pub autostart: bool,
}

fn default_lang() -> String {
    "en".into()
}
fn default_profile() -> String {
    "balanced".into()
}
fn default_fps() -> u32 {
    15
}
fn default_max_tiles() -> usize {
    4
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: default_lang(),
            profile: default_profile(),
            render_fps: default_fps(),
            max_live_tiles: default_max_tiles(),
            autostart: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_storage")]
    pub dir: String,
    #[serde(default = "default_min_free")]
    pub min_free_mb: u64,
}

fn default_storage() -> String {
    platform::default_storage_dir()
        .to_string_lossy()
        .into_owned()
}
fn default_min_free() -> u64 {
    500
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            dir: default_storage(),
            min_free_mb: default_min_free(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecordingConfig {
    /// Maximum total size of all CamView recordings. 0 means unlimited.
    #[serde(default = "default_max_storage_mb")]
    pub max_storage_mb: u64,
    /// Length of each recording segment before it is rotated.
    #[serde(default = "default_segment_seconds")]
    pub segment_seconds: u64,
}

fn default_max_storage_mb() -> u64 {
    1024
}
fn default_segment_seconds() -> u64 {
    120
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            max_storage_mb: default_max_storage_mb(),
            segment_seconds: default_segment_seconds(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub app: AppConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub recording: RecordingConfig,
    #[serde(default)]
    pub cameras: Vec<Camera>,
}

impl Config {
    pub fn load_or_import() -> (Self, Option<String>) {
        let path = platform::config_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(text) => match toml::from_str::<Config>(&text) {
                    Ok(cfg) => return (cfg, None),
                    Err(e) => {
                        platform::log_line(&format!("Error leyendo config: {e}"));
                        return (
                            Config::default(),
                            Some(format!("Invalid config: {e}. Using defaults.")),
                        );
                    }
                },
                Err(e) => {
                    platform::log_line(&format!("Could not read config: {e}"));
                }
            }
        }

        match import_from_m3u() {
            Some((cfg, note)) => {
                if let Err(e) = cfg.save() {
                    platform::log_line(&format!("Could not save imported config: {e}"));
                }
                (cfg, Some(note))
            }
            None => {
                let cfg = Config::default();
                let _ = cfg.save();
                (cfg, None)
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = platform::config_dir();
        platform::ensure_dir(&dir)?;
        let text = toml::to_string_pretty(self).map_err(|e| format!("TOML: {e}"))?;
        let path = platform::config_path();
        let tmp = dir.join("config.toml.tmp");
        fs::write(&tmp, text).map_err(|e| format!("Could not write config: {e}"))?;
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("Config permissions: {e}"))?;
        }
        if path.exists() {
            let _ = fs::copy(&path, dir.join("config.toml.bak"));
        }
        fs::rename(&tmp, &path).map_err(|e| format!("Could not save config: {e}"))?;
        Ok(())
    }

    pub fn camera(&self, id: &str) -> Option<&Camera> {
        self.cameras.iter().find(|c| c.id == id)
    }

    pub fn camera_mut(&mut self, id: &str) -> Option<&mut Camera> {
        self.cameras.iter_mut().find(|c| c.id == id)
    }
}

// ---------------------------------------------------------------------------
// READ-ONLY import from a legacy M3U playlist and systemd proxy units.
// Never modifies the source playlist or services.
// ---------------------------------------------------------------------------

fn take_until<'a>(s: &'a str, stops: &[char]) -> &'a str {
    s.split(|c| stops.contains(&c)).next().unwrap_or("")
}

fn after<'a>(s: &'a str, needle: &str) -> Option<&'a str> {
    s.find(needle).map(|i| &s[i + needle.len()..])
}

fn read_proxy_units() -> HashMap<u16, (String, u16)> {
    let mut map = HashMap::new();
    let dir = platform::home().join(".config/systemd/user");
    let Ok(entries) = fs::read_dir(dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("service") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(listen) = after(&text, "TCP-LISTEN:") else {
            continue;
        };
        let local_port = take_until(listen, &[',', ' ', '\n']).parse::<u16>().ok();
        let Some(ssl) = after(&text, "OPENSSL:") else {
            continue;
        };
        let upstream = take_until(ssl, &[',', ' ', '\n']);
        let Some((host, port)) = upstream.rsplit_once(':') else {
            continue;
        };
        let Ok(port) = port.parse::<u16>() else {
            continue;
        };
        if let Some(local_port) = local_port {
            map.insert(local_port, (host.to_string(), port));
        }
    }
    map
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

pub fn import_from_m3u() -> Option<(Config, String)> {
    let home = platform::home();
    let m3u_path = [home.join("cameras.m3u"), home.join("camaras.m3u")]
        .into_iter()
        .find(|p| p.exists())?;
    let text = fs::read_to_string(&m3u_path).ok()?;
    let units = read_proxy_units();
    let mut cameras: Vec<Camera> = Vec::new();
    let mut pending_name: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF") {
            let name = after(rest, "tvg-name=\"")
                .and_then(|s| s.split('"').next())
                .map(|s| s.to_string())
                .or_else(|| rest.rsplit_once(',').map(|(_, n)| n.trim().to_string()))
                .unwrap_or_else(|| "Camera".into());
            pending_name = Some(name);
            continue;
        }
        if line.starts_with('#') || !line.contains("://") {
            continue;
        }
        let Some(name) = pending_name.take() else {
            continue;
        };
        let Ok(url) = Url::parse(line) else {
            continue;
        };
        let scheme = url.scheme().to_string();
        let host_is_loopback = matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"));
        let url_port = url
            .port()
            .unwrap_or(if scheme == "rtsps" { 322 } else { 554 });
        let path = url.path().to_string();
        let user = percent_decode(url.username());
        let password = url.password().map(percent_decode).unwrap_or_default();

        let (host, rtsp_port, tls) = if host_is_loopback {
            match units.get(&url_port) {
                Some((h, p)) => (h.clone(), *p, true),
                None => {
                    platform::log_line(&format!(
                        "Import: no proxy unit found for 127.0.0.1:{url_port}; skipping {name}"
                    ));
                    continue;
                }
            }
        } else {
            (
                url.host_str().unwrap_or("").to_string(),
                url_port,
                scheme == "rtsps",
            )
        };

        let (main_path, sub_path) = if path.contains("stream0") {
            (path.clone(), "/stream1".to_string())
        } else if path.contains("stream1") {
            ("/stream0".to_string(), path.clone())
        } else {
            ("/stream0".to_string(), "/stream1".to_string())
        };

        let mut id = platform::sanitize_id(&name);
        let mut n = 2;
        while cameras.iter().any(|c| c.id == id) {
            id = format!("{}-{n}", platform::sanitize_id(&name));
            n += 1;
        }

        cameras.push(Camera {
            id,
            name,
            host,
            tls,
            rtsp_port,
            user,
            password,
            main_path,
            sub_path,
            enabled: true,
        });
    }

    if cameras.is_empty() {
        return None;
    }

    let mut cfg = Config::default();
    cfg.cameras = cameras;
    let n = cfg.cameras.len();
    Some((cfg, format!("{n} cameras imported from M3U (read-only)")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Camera {
        Camera {
            id: "front-door".into(),
            name: "Front door".into(),
            host: "192.0.2.10".into(),
            tls: true,
            rtsp_port: 554,
            user: "viewer".into(),
            password: "p@ss".into(),
            main_path: "/stream0".into(),
            sub_path: "/stream1".into(),
            enabled: true,
        }
    }

    #[test]
    fn builds_encoded_local_url_for_tls_camera() {
        let cam = sample();
        assert_eq!(
            cam.url_for(Some(9000), StreamKind::Sub),
            "rtsp://viewer:p%40ss@127.0.0.1:9000/stream1"
        );
        assert_eq!(
            cam.url_for(Some(9000), StreamKind::Main),
            "rtsp://viewer:p%40ss@127.0.0.1:9000/stream0"
        );
    }

    #[test]
    fn builds_direct_url_for_plain_rtsp_camera() {
        let mut cam = sample();
        cam.tls = false;
        cam.rtsp_port = 554;
        assert_eq!(
            cam.url_for(None, StreamKind::Sub),
            "rtsp://viewer:p%40ss@192.0.2.10:554/stream1"
        );
    }

    #[test]
    fn config_round_trips_through_toml() {
        let cfg = Config {
            app: AppConfig::default(),
            storage: StorageConfig {
                dir: "/tmp/camview".into(),
                min_free_mb: 500,
            },
            recording: RecordingConfig {
                max_storage_mb: 2048,
                segment_seconds: 120,
            },
            cameras: vec![sample()],
        };
        let text = toml::to_string(&cfg).expect("serialize");
        let back: Config = toml::from_str(&text).expect("deserialize");
        assert_eq!(back.cameras.len(), 1);
        assert_eq!(back.cameras[0].id, "front-door");
        assert_eq!(back.app.render_fps, 15);
        assert_eq!(back.recording.max_storage_mb, 2048);
    }

    #[test]
    fn legacy_config_without_recording_uses_default_limit() {
        let text = r#"
[app]
language = "en"

[storage]
dir = "/tmp/camview"
min_free_mb = 500

[[cameras]]
id = "front-door"
name = "Front door"
host = "192.0.2.10"
"#;
        let cfg: Config = toml::from_str(text).expect("deserialize legacy config");
        assert_eq!(cfg.recording.max_storage_mb, 1024);
        assert_eq!(cfg.recording.segment_seconds, 120);
        assert_eq!(cfg.cameras.len(), 1);
    }
}
