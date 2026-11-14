use std::collections::HashMap;
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::config::Camera;
use crate::platform;

struct Tunnel {
    child: Child,
    port: u16,
}

pub struct TunnelManager {
    tunnels: HashMap<String, Tunnel>,
    errors: HashMap<String, String>,
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            tunnels: HashMap::new(),
            errors: HashMap::new(),
        }
    }

    pub fn port_for(&self, id: &str) -> Option<u16> {
        self.tunnels.get(id).map(|t| t.port)
    }

    pub fn error_for(&self, id: &str) -> Option<&str> {
        self.errors.get(id).map(|s| s.as_str())
    }

    /// Starts the tunnel if the camera uses TLS. Returns the local port or None for plain RTSP.
    pub fn ensure(&mut self, cam: &Camera) -> Result<Option<u16>, String> {
        if !cam.tls {
            return Ok(None);
        }
        if let Some(t) = self.tunnels.get_mut(&cam.id) {
            match t.child.try_wait() {
                Ok(None) => return Ok(Some(t.port)),
                _ => {
                    self.tunnels.remove(&cam.id);
                }
            }
        }
        let port = self.start(cam, None)?;
        Ok(Some(port))
    }

    fn start(&mut self, cam: &Camera, preferred: Option<u16>) -> Result<u16, String> {
        if cam.host.trim().is_empty() {
            return Err("Camera IP/host is missing".into());
        }
        let mut port = match preferred {
            Some(p) => p,
            None => free_port()?,
        };
        let mut last_err = String::new();
        for attempt in 0..3 {
            match spawn_socat(port, cam) {
                Ok(child) => {
                    platform::log_line(&format!(
                        "Tunnel {} listening on 127.0.0.1:{} -> {}:{}",
                        cam.id, port, cam.host, cam.rtsp_port
                    ));
                    self.tunnels.insert(cam.id.clone(), Tunnel { child, port });
                    self.errors.remove(&cam.id);
                    return Ok(port);
                }
                Err(e) => {
                    last_err = e;
                    if attempt < 2 {
                        port = free_port()?;
                    }
                }
            }
        }
        let msg = format!("Could not open tunnel for {}: {last_err}", cam.name);
        platform::log_line(&msg);
        self.errors.insert(cam.id.clone(), msg.clone());
        Err(msg)
    }

    /// Checks the socat processes. Returns port changes that require reloading the player.
    pub fn check_and_restart(&mut self, cameras: &[Camera]) -> Vec<(String, u16, u16)> {
        let mut changed = Vec::new();
        for cam in cameras.iter().filter(|c| c.enabled && c.tls) {
            let alive = self
                .tunnels
                .get_mut(&cam.id)
                .map(|t| matches!(t.child.try_wait(), Ok(None)))
                .unwrap_or(false);
            if alive {
                continue;
            }
            let old = self.tunnels.remove(&cam.id).map(|mut t| {
                let _ = t.child.kill();
                let _ = t.child.wait();
                t.port
            });
            if old.is_some() {
                platform::log_line(&format!("Tunnel {} died; restarting", cam.id));
            }
            match self.start(cam, old) {
                Ok(new_port) => {
                    if let Some(old_port) = old {
                        if old_port != new_port {
                            changed.push((cam.id.clone(), old_port, new_port));
                        }
                    }
                }
                Err(_) => {}
            }
        }
        changed
    }

    pub fn stop(&mut self, id: &str) {
        if let Some(mut t) = self.tunnels.remove(id) {
            let _ = t.child.kill();
            let _ = t.child.wait();
        }
    }
}

impl Drop for TunnelManager {
    fn drop(&mut self) {
        for (_, mut t) in self.tunnels.drain() {
            let _ = t.child.kill();
            let _ = t.child.wait();
        }
    }
}

fn free_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("No free local port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Could not read local port: {e}"))?
        .port();
    drop(listener);
    Ok(port)
}

fn spawn_socat(port: u16, cam: &Camera) -> Result<Child, String> {
    let listen = format!("TCP-LISTEN:{port},reuseaddr,fork,bind=127.0.0.1");
    let upstream = format!("OPENSSL:{}:{},verify=0", cam.host, cam.rtsp_port);
    let mut cmd = Command::new("socat");
    cmd.arg(listen)
        .arg(upstream)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "socat is not installed (sudo apt install socat)".to_string()
        } else {
            format!("socat: {e}")
        }
    })?;
    std::thread::sleep(Duration::from_millis(180));
    match child.try_wait() {
        Ok(None) => Ok(child),
        Ok(Some(status)) => Err(format!("socat exited on startup ({status})")),
        Err(e) => Err(format!("socat: {e}")),
    }
}
