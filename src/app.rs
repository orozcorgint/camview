use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Margin, PointerButton, Rect, RichText, Sense,
    Stroke, StrokeKind, Vec2,
};
use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

use crate::config::{Camera, Config, StreamKind};
use crate::gltile::TileGl;
use crate::platform;
use crate::player::{Player, PlayerState};
use crate::storage;
use crate::tunnel::TunnelManager;
use crate::zoom::{self, ZoomView};

const BG: Color32 = Color32::from_rgb(11, 13, 17);
const PANEL: Color32 = Color32::from_rgb(18, 21, 28);
const CARD: Color32 = Color32::from_rgb(22, 26, 34);
const CARD_HOVER: Color32 = Color32::from_rgb(30, 35, 45);
const ACCENT: Color32 = Color32::from_rgb(70, 190, 255);
const TEXT: Color32 = Color32::from_rgb(224, 230, 240);
const MUTED: Color32 = Color32::from_rgb(145, 155, 170);
const GREEN: Color32 = Color32::from_rgb(60, 210, 130);
const YELLOW: Color32 = Color32::from_rgb(235, 190, 70);
const ORANGE: Color32 = Color32::from_rgb(240, 140, 60);
const RED: Color32 = Color32::from_rgb(235, 80, 80);

enum UiAction {
    Select(String),
    Fullscreen(Option<String>),
    Snapshot(String),
    ToggleAudio(String),
    ToggleRecord(String),
    RecordAll,
    StopAll,
    AddCamera,
    EditCamera(String),
    DeleteCamera(String),
    ConfirmDelete(String),
    CancelDelete,
    TestCamera(String),
    ToggleEnabled(String, bool),
}

#[derive(Clone)]
struct TileInfo {
    state: PlayerState,
    width: u32,
    height: u32,
    aspect: f32,
    hwdec: String,
    recording: bool,
    record_desired: bool,
    audio: bool,
    record_secs: u64,
}

impl Default for TileInfo {
    fn default() -> Self {
        Self {
            state: PlayerState::Idle,
            width: 0,
            height: 0,
            aspect: 16.0 / 9.0,
            hwdec: String::new(),
            recording: false,
            record_desired: false,
            audio: false,
            record_secs: 0,
        }
    }
}

struct Probe {
    rx: Receiver<String>,
}

struct CameraForm {
    editing: bool,
    original_id: Option<String>,
    name: String,
    host: String,
    port: String,
    user: String,
    password: String,
    main_path: String,
    sub_path: String,
    tls: bool,
    enabled: bool,
    error: Option<String>,
}

impl CameraForm {
    fn new() -> Self {
        Self {
            editing: false,
            original_id: None,
            name: String::new(),
            host: String::new(),
            port: "554".into(),
            user: String::new(),
            password: String::new(),
            main_path: "/stream0".into(),
            sub_path: "/stream1".into(),
            tls: true,
            enabled: true,
            error: None,
        }
    }

    fn from_camera(cam: &Camera) -> Self {
        Self {
            editing: true,
            original_id: Some(cam.id.clone()),
            name: cam.name.clone(),
            host: cam.host.clone(),
            port: cam.rtsp_port.to_string(),
            user: cam.user.clone(),
            password: cam.password.clone(),
            main_path: cam.main_path.clone(),
            sub_path: cam.sub_path.clone(),
            tls: cam.tls,
            enabled: cam.enabled,
            error: None,
        }
    }
}

pub struct CamViewApp {
    cfg: Config,
    tunnels: TunnelManager,
    players: HashMap<String, Arc<Mutex<Player>>>,
    // Kept per camera: GL resources cannot be safely deleted in app logic, and
    // reusing them avoids leaks when players are recreated on page changes.
    gl_tiles: HashMap<String, Arc<Mutex<TileGl>>>,
    player_stream: HashMap<String, StreamKind>,
    x11_display: Option<usize>,
    selected: Option<String>,
    fullscreen: Option<String>,
    page: usize,
    show_cameras: bool,
    show_settings: bool,
    camera_form: Option<CameraForm>,
    delete_confirm: Option<String>,
    probe: Option<Probe>,
    probe_text: String,
    toast: Option<(String, Instant)>,
    fatal: Option<String>,
    disk_free: u64,
    mem_avail: u64,
    record_intent: HashSet<String>,
    record_usage_bytes: u64,
    last_record_retry: HashMap<String, Instant>,
    zoom: ZoomView,
    zoom_video_rect: Rect,
    zoom_content_rect: Rect,
    dirty: bool,
    needs_sync: bool,
    last_tunnel_check: Instant,
    last_disk_check: Instant,
    last_storage_check: Instant,
    storage_prune_now: bool,
    last_save: Instant,
}

impl CamViewApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (cfg, note) = Config::load_or_import();
        if let Some(note) = &note {
            platform::log_line(note);
        }
        let mut tunnels = TunnelManager::new();
        for cam in cfg.cameras.iter().filter(|c| c.enabled) {
            if let Err(e) = tunnels.ensure(cam) {
                platform::log_line(&e);
            }
        }
        let _ = platform::ensure_dir(Path::new(&cfg.storage.dir));

        let x11_display = cc.display_handle().ok().and_then(|h| match h.as_raw() {
            RawDisplayHandle::Xlib(h) => h.display.map(|p| p.as_ptr() as usize),
            _ => None,
        });
        let disk_free = platform::disk_free_mb(Path::new(&cfg.storage.dir)).unwrap_or(0);
        let record_usage_bytes = storage::usage_bytes(Path::new(&cfg.storage.dir));

        let app = Self {
            selected: cfg.cameras.iter().find(|c| c.enabled).map(|c| c.id.clone()),
            cfg,
            tunnels,
            players: HashMap::new(),
            gl_tiles: HashMap::new(),
            player_stream: HashMap::new(),
            x11_display,
            fullscreen: None,
            page: 0,
            show_cameras: false,
            show_settings: false,
            camera_form: None,
            delete_confirm: None,
            probe: None,
            probe_text: String::new(),
            toast: note.map(|n| (n, Instant::now())),
            fatal: None,
            disk_free,
            mem_avail: platform::mem_available_mb().unwrap_or(0),
            record_intent: HashSet::new(),
            record_usage_bytes,
            last_record_retry: HashMap::new(),
            zoom: ZoomView::default(),
            zoom_video_rect: Rect::from_min_size(egui::Pos2::ZERO, Vec2::ZERO),
            zoom_content_rect: Rect::from_min_size(egui::Pos2::ZERO, Vec2::ZERO),
            dirty: false,
            needs_sync: true,
            last_tunnel_check: Instant::now(),
            last_disk_check: Instant::now() - Duration::from_secs(10),
            last_storage_check: Instant::now() - Duration::from_secs(10),
            storage_prune_now: false,
            last_save: Instant::now(),
        };

        if app.cfg.app.autostart != platform::is_autostart_enabled() {
            let _ = platform::set_autostart(app.cfg.app.autostart);
        }
        app.apply_theme(&cc.egui_ctx);
        app
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        // Ctrl +/- must zoom the fullscreen video, not egui's whole UI.
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        ctx.set_zoom_factor(1.0);

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = PANEL;
        visuals.extreme_bg_color = Color32::from_rgb(10, 12, 16);
        visuals.faint_bg_color = CARD;
        visuals.override_text_color = Some(TEXT);
        visuals.selection.bg_fill = ACCENT;
        visuals.widgets.noninteractive.bg_fill = PANEL;
        visuals.widgets.inactive.bg_fill = CARD;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, MUTED);
        visuals.widgets.hovered.bg_fill = CARD_HOVER;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        visuals.widgets.active.bg_fill = ACCENT;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::BLACK);
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(20.0));
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(11.0));
        ctx.set_style(style);
    }

    fn toast(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        platform::log_line(&msg);
        self.toast = Some((msg, Instant::now()));
    }

    fn pages(&self) -> usize {
        let per = self.cfg.app.max_live_tiles.max(1);
        let enabled = self.cfg.cameras.iter().filter(|c| c.enabled).count();
        ((enabled + per - 1) / per).max(1)
    }

    fn clamp_page(&mut self) {
        let pages = self.pages();
        if self.page >= pages {
            self.page = pages - 1;
            self.needs_sync = true;
        }
    }

    fn page_cameras(&self) -> Vec<Camera> {
        let per = self.cfg.app.max_live_tiles.max(1);
        let enabled: Vec<Camera> = self
            .cfg
            .cameras
            .iter()
            .filter(|c| c.enabled)
            .cloned()
            .collect();
        enabled
            .into_iter()
            .skip(self.page * per)
            .take(per)
            .collect()
    }

    fn player_info(&self, id: &str) -> TileInfo {
        let desired = self.record_intent.contains(id);
        let Some(player) = self.players.get(id) else {
            return TileInfo::default();
        };
        let Ok(p) = player.lock() else {
            return TileInfo::default();
        };
        TileInfo {
            state: p.state.clone(),
            width: p.width,
            height: p.height,
            aspect: p.aspect,
            hwdec: p.hwdec.clone(),
            recording: p.is_recording(),
            record_desired: p.is_recording() || desired,
            audio: p.audio,
            record_secs: p.record_started.map(|t| t.elapsed().as_secs()).unwrap_or(0),
        }
    }

    fn any_recording(&self) -> bool {
        !self.record_intent.is_empty()
            || self
                .players
                .values()
                .any(|p| p.lock().map(|p| p.is_recording()).unwrap_or(false))
    }

    fn sync_players(&mut self) {
        self.clamp_page();
        let desired: Vec<Camera> = if let Some(full) = &self.fullscreen {
            self.cfg
                .cameras
                .iter()
                .filter(|c| c.enabled && c.id == *full)
                .cloned()
                .collect()
        } else {
            self.page_cameras()
        };
        let desired_ids: std::collections::HashSet<String> =
            desired.iter().map(|c| c.id.clone()).collect();

        let existing: Vec<String> = self.players.keys().cloned().collect();
        for id in existing {
            if desired_ids.contains(&id) {
                continue;
            }
            let keep = self.record_intent.contains(&id)
                || self
                    .players
                    .get(&id)
                    .and_then(|p| p.lock().ok().map(|p| p.is_recording()))
                    .unwrap_or(false);
            if !keep {
                if let Some(player) = self.players.remove(&id) {
                    if let Ok(mut p) = player.lock() {
                        p.stop();
                    }
                }
                self.player_stream.remove(&id);
                self.tunnels.stop(&id);
            }
        }

        for cam in desired {
            if self.players.contains_key(&cam.id) {
                continue;
            }
            if let Err(e) = self.tunnels.ensure(&cam) {
                self.toast(e);
                continue;
            }
            let port = self.tunnels.port_for(&cam.id);
            let kind = if self.fullscreen.as_deref() == Some(cam.id.as_str()) {
                StreamKind::Main
            } else {
                StreamKind::Sub
            };
            let url = cam.url_for(port, kind);
            match Player::new(&cam.id, self.x11_display) {
                Ok(mut player) => {
                    player.load(url);
                    self.gl_tiles
                        .entry(cam.id.clone())
                        .or_insert_with(|| Arc::new(Mutex::new(TileGl::default())));
                    self.players
                        .insert(cam.id.clone(), Arc::new(Mutex::new(player)));
                    self.player_stream.insert(cam.id.clone(), kind);
                }
                Err(e) => {
                    self.toast(format!("{}: {e}", cam.name));
                    platform::log_line(&format!("{}: {e}", cam.name));
                }
            }
        }
    }

    fn switch_stream(&mut self, id: &str, kind: StreamKind) {
        let Some(cam) = self.cfg.camera(id).cloned() else {
            return;
        };
        let Some(player) = self.players.get(id).cloned() else {
            return;
        };
        let recording = player.lock().map(|p| p.is_recording()).unwrap_or(false);
        if recording {
            self.toast("Recording: the current stream is kept until you stop recording");
            return;
        }
        let _ = self.tunnels.ensure(&cam);
        let port = self.tunnels.port_for(id);
        let url = cam.url_for(port, kind);
        if let Ok(mut p) = player.lock() {
            p.load(url);
        }
        self.player_stream.insert(id.to_string(), kind);
    }

    fn set_fullscreen(&mut self, id: Option<String>) {
        match id {
            Some(id) => {
                if self.fullscreen.as_deref() != Some(id.as_str()) {
                    self.zoom.reset();
                    self.fullscreen = Some(id.clone());
                    self.selected = Some(id.clone());
                    self.switch_stream(&id, StreamKind::Main);
                    self.sync_players();
                }
            }
            None => {
                if let Some(id) = self.fullscreen.take() {
                    self.zoom.reset();
                    self.switch_stream(&id, StreamKind::Sub);
                    self.sync_players();
                }
            }
        }
    }

    fn adjust_zoom(&mut self, factor: f32) {
        if self.zoom_video_rect.width() <= 0.0 {
            return;
        }
        self.zoom
            .zoom_center(self.zoom_video_rect, self.zoom_content_rect, factor);
    }

    fn set_profile(&mut self, profile: &str) {
        if self.cfg.app.profile == profile {
            return;
        }
        self.cfg.app.profile = profile.to_string();
        self.cfg.app.render_fps = match profile {
            "eco" => 5,
            _ => 15,
        };
        self.dirty = true;
    }

    fn start_recording_for(&mut self, id: &str, announce: bool) -> bool {
        if self.record_intent.contains(id) {
            return true;
        }
        let Some(cam) = self.cfg.camera(id).cloned() else {
            return false;
        };
        if self.disk_free > 0 && self.disk_free < self.cfg.storage.min_free_mb {
            if announce {
                self.toast("Low disk space: recording cancelled");
            }
            return false;
        }
        let Some(player) = self.players.get(id).cloned() else {
            if announce {
                self.toast("Camera is not loaded");
            }
            return false;
        };
        if player.lock().map(|p| p.is_recording()).unwrap_or(false) {
            return true;
        }

        self.record_intent.insert(id.to_string());
        self.last_record_retry.remove(id);
        let live = player.lock().map(|p| p.state.is_live()).unwrap_or(false);
        if !live {
            if announce {
                self.toast("Recording will start when the camera is live");
            }
            return true;
        }

        let storage = PathBuf::from(&self.cfg.storage.dir);
        let mut p = match player.lock() {
            Ok(p) => p,
            Err(_) => return true,
        };
        match p.start_recording(&storage, &cam.slug()) {
            Ok(path) => {
                if announce {
                    self.toast(format!("Recording {} → {}", cam.name, path.display()));
                }
                true
            }
            Err(e) => {
                if announce {
                    self.toast(e);
                } else {
                    platform::log_line(&format!("{}: {e}", cam.name));
                }
                self.last_record_retry
                    .insert(id.to_string(), Instant::now() + Duration::from_secs(10));
                false
            }
        }
    }

    fn stop_recording(&mut self, id: &str) {
        self.record_intent.remove(id);
        self.last_record_retry.remove(id);
        let Some(player) = self.players.get(id).cloned() else {
            return;
        };
        let path = player.lock().ok().and_then(|mut p| p.stop_recording());
        if let Some(path) = path {
            self.toast(format!("Recording saved: {}", path.display()));
            let is_full = self.fullscreen.as_deref() == Some(id);
            let current = self
                .player_stream
                .get(id)
                .copied()
                .unwrap_or(StreamKind::Sub);
            if !is_full && current == StreamKind::Main {
                self.switch_stream(id, StreamKind::Sub);
            }
        }
        self.needs_sync = true;
    }

    fn stop_all_recordings(&mut self) {
        self.record_intent.clear();
        self.last_record_retry.clear();
        let ids: Vec<String> = self.players.keys().cloned().collect();
        for id in ids {
            if let Some(p) = self.players.get(&id) {
                if let Ok(mut p) = p.lock() {
                    let _ = p.stop_recording();
                }
            }
        }
        self.needs_sync = true;
    }

    fn active_recording_paths(&self) -> HashSet<PathBuf> {
        self.players
            .values()
            .filter_map(|p| {
                p.lock()
                    .ok()
                    .and_then(|p| p.recording_path().map(Path::to_path_buf))
            })
            .collect()
    }

    fn refresh_storage(&mut self, prune: bool) {
        let root = PathBuf::from(&self.cfg.storage.dir);
        let active = self.active_recording_paths();
        if prune && self.cfg.recording.max_storage_mb > 0 {
            let max_bytes = self
                .cfg
                .recording
                .max_storage_mb
                .saturating_mul(1024 * 1024);
            let target = max_bytes.saturating_sub(storage::SEGMENT_MAX_BYTES).max(1);
            match storage::prune(&root, target, &active) {
                Ok(report) if report.deleted_files > 0 => {
                    platform::log_line(&format!(
                        "Recording storage: removed {} clips ({})",
                        report.deleted_files,
                        platform::format_mb(report.deleted_bytes / (1024 * 1024))
                    ));
                }
                Ok(_) => {}
                Err(e) => platform::log_line(&format!("Recording storage: {e}")),
            }
        }
        self.record_usage_bytes = storage::usage_bytes(&root);
    }

    fn maintain_recordings(&mut self) {
        let segment_secs = self.cfg.recording.segment_seconds.max(30);
        let ids: Vec<String> = self.record_intent.iter().cloned().collect();
        let mut rotated = false;

        for id in ids {
            let Some(cam) = self.cfg.camera(&id).cloned() else {
                self.record_intent.remove(&id);
                continue;
            };
            if !cam.enabled {
                self.record_intent.remove(&id);
                continue;
            }
            let Some(player) = self.players.get(&id).cloned() else {
                continue;
            };

            let info = match player.lock() {
                Ok(p) => (
                    p.is_recording(),
                    p.state.is_live(),
                    p.segment_elapsed(),
                    p.recording_path()
                        .and_then(|path| std::fs::metadata(path).ok())
                        .map(|meta| meta.len()),
                ),
                Err(_) => continue,
            };

            if info.0 {
                let rotate_by_time = info
                    .2
                    .map(|elapsed| elapsed >= Duration::from_secs(segment_secs))
                    .unwrap_or(false);
                let rotate_by_size = info
                    .3
                    .map(|size| size >= storage::SEGMENT_MAX_BYTES)
                    .unwrap_or(false);
                if rotate_by_time || rotate_by_size {
                    let storage = PathBuf::from(&self.cfg.storage.dir);
                    let result = player
                        .lock()
                        .map(|mut p| p.rotate_recording(&storage, &cam.slug()));
                    match result {
                        Ok(Ok(_)) => rotated = true,
                        Ok(Err(e)) => {
                            platform::log_line(&format!("{}: {e}", cam.name));
                            if let Ok(mut p) = player.lock() {
                                let _ = p.stop_recording();
                            }
                            self.last_record_retry
                                .insert(id.clone(), Instant::now() + Duration::from_secs(5));
                        }
                        Err(_) => {}
                    }
                }
            } else if info.1 {
                let retry = self
                    .last_record_retry
                    .get(&id)
                    .map(|at| *at <= Instant::now())
                    .unwrap_or(true);
                if retry {
                    self.start_recording_for(&id, false);
                }
            }
        }

        if rotated {
            self.refresh_storage(true);
            self.last_storage_check = Instant::now();
        }
    }

    fn apply_action(&mut self, action: UiAction) {
        match action {
            UiAction::Select(id) => {
                self.selected = Some(id);
            }
            UiAction::Fullscreen(id) => self.set_fullscreen(id),
            UiAction::Snapshot(id) => {
                let Some(cam) = self.cfg.camera(&id).cloned() else {
                    return;
                };
                let Some(player) = self.players.get(&id).cloned() else {
                    self.toast("Camera is not loaded");
                    return;
                };
                let shot = match player.lock() {
                    Ok(p) => p.snapshot(&cam.name),
                    Err(_) => Err("Could not access the player".to_string()),
                };
                match shot {
                    Ok(path) => self.toast(format!("Snapshot: {}", path.display())),
                    Err(e) => self.toast(e),
                }
            }
            UiAction::ToggleAudio(id) => {
                let target_audio = self.player_info(&id).audio;
                let ids: Vec<String> = self.players.keys().cloned().collect();
                for pid in ids {
                    if let Some(p) = self.players.get(&pid) {
                        if let Ok(mut p) = p.lock() {
                            p.set_audio(false);
                        }
                    }
                }
                if !target_audio {
                    if let Some(p) = self.players.get(&id) {
                        if let Ok(mut p) = p.lock() {
                            p.set_audio(true);
                        }
                    }
                    self.toast("Audio enabled for this camera");
                } else {
                    self.toast("Audio muted");
                }
            }
            UiAction::ToggleRecord(id) => {
                if self.record_intent.contains(&id) || self.player_info(&id).recording {
                    self.stop_recording(&id);
                } else {
                    self.start_recording_for(&id, true);
                }
            }
            UiAction::RecordAll => {
                let ids: Vec<String> = self.players.keys().cloned().collect();
                let mut count = 0;
                for id in ids {
                    if self.start_recording_for(&id, false) {
                        count += 1;
                    }
                }
                if count == 0 {
                    self.toast("No cameras available to record");
                } else {
                    self.toast(format!("Recording {count} cameras"));
                }
            }
            UiAction::StopAll => {
                self.stop_all_recordings();
                self.toast("Recordings stopped");
            }
            UiAction::AddCamera => {
                self.camera_form = Some(CameraForm::new());
            }
            UiAction::EditCamera(id) => {
                if let Some(cam) = self.cfg.camera(&id) {
                    self.camera_form = Some(CameraForm::from_camera(cam));
                }
            }
            UiAction::DeleteCamera(id) => {
                self.delete_confirm = Some(id);
            }
            UiAction::ConfirmDelete(id) => {
                self.delete_camera(&id);
                self.delete_confirm = None;
            }
            UiAction::CancelDelete => {
                self.delete_confirm = None;
            }
            UiAction::TestCamera(id) => {
                if let Some(cam) = self.cfg.camera(&id).cloned() {
                    self.start_probe(&cam);
                }
            }
            UiAction::ToggleEnabled(id, enabled) => {
                if let Some(cam) = self.cfg.camera_mut(&id) {
                    cam.enabled = enabled;
                }
                if !enabled {
                    self.record_intent.remove(&id);
                    self.last_record_retry.remove(&id);
                    self.tunnels.stop(&id);
                    if let Some(p) = self.players.remove(&id) {
                        if let Ok(mut p) = p.lock() {
                            p.stop();
                        }
                    }
                    self.player_stream.remove(&id);
                    if self.selected.as_deref() == Some(id.as_str()) {
                        self.selected = self
                            .cfg
                            .cameras
                            .iter()
                            .find(|c| c.enabled)
                            .map(|c| c.id.clone());
                    }
                    if self.fullscreen.as_deref() == Some(id.as_str()) {
                        self.fullscreen = None;
                    }
                }
                self.needs_sync = true;
                self.dirty = true;
            }
        }
    }

    fn delete_camera(&mut self, id: &str) {
        self.record_intent.remove(id);
        self.last_record_retry.remove(id);
        self.tunnels.stop(id);
        if let Some(p) = self.players.remove(id) {
            if let Ok(mut p) = p.lock() {
                p.stop();
            }
        }
        self.player_stream.remove(id);
        self.cfg.cameras.retain(|c| c.id != id);
        if self.selected.as_deref() == Some(id) {
            self.selected = self
                .cfg
                .cameras
                .iter()
                .find(|c| c.enabled)
                .map(|c| c.id.clone());
        }
        if self.fullscreen.as_deref() == Some(id) {
            self.fullscreen = None;
        }
        self.dirty = true;
        self.needs_sync = true;
        self.toast("Camera deleted");
    }

    fn commit_camera(&mut self, form: &CameraForm) -> Result<(), String> {
        let name = form.name.trim();
        if name.is_empty() {
            return Err("Name cannot be empty".into());
        }
        let host = form.host.trim();
        if host.is_empty() {
            return Err("IP/host cannot be empty".into());
        }
        let port: u16 = form
            .port
            .trim()
            .parse()
            .map_err(|_| "Invalid port".to_string())?;
        if form.main_path.trim().is_empty() || form.sub_path.trim().is_empty() {
            return Err("Main/sub paths cannot be empty".into());
        }

        let mut cam = Camera {
            id: form
                .original_id
                .clone()
                .unwrap_or_else(|| platform::sanitize_id(name)),
            name: name.to_string(),
            host: host.to_string(),
            tls: form.tls,
            rtsp_port: port,
            user: form.user.clone(),
            password: form.password.clone(),
            main_path: form.main_path.trim().to_string(),
            sub_path: form.sub_path.trim().to_string(),
            enabled: form.enabled,
        };

        if form.original_id.is_none() {
            let base = cam.id.clone();
            let mut n = 2;
            while self.cfg.cameras.iter().any(|c| c.id == cam.id) {
                cam.id = format!("{base}-{n}");
                n += 1;
            }
        }

        if let Some(orig) = &form.original_id {
            let Some(existing) = self.cfg.camera_mut(orig) else {
                return Err("Camera no longer exists".into());
            };
            *existing = cam;
            self.tunnels.stop(orig);
            if let Some(p) = self.players.remove(orig) {
                if let Ok(mut p) = p.lock() {
                    p.stop();
                }
            }
            self.player_stream.remove(orig);
        } else {
            self.cfg.cameras.push(cam.clone());
            self.selected = Some(cam.id.clone());
        }

        self.cfg.save()?;
        self.needs_sync = true;
        self.toast(if form.editing {
            "Camera updated"
        } else {
            "Camera added"
        });
        Ok(())
    }

    fn start_probe(&mut self, cam: &Camera) {
        let port = match self.tunnels.ensure(cam) {
            Ok(p) => p,
            Err(e) => {
                self.toast(e);
                return;
            }
        };
        let url = cam.url_for(port, StreamKind::Sub);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = std::process::Command::new("ffprobe")
                .args([
                    "-rtsp_transport",
                    "tcp",
                    "-timeout",
                    "5000000",
                    "-v",
                    "error",
                    "-show_entries",
                    "stream=codec_name,width,height",
                    "-of",
                    "default=noprint_wrappers=1",
                ])
                .arg(&url)
                .output();
            let text = match out {
                Ok(o) if o.status.success() => {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if s.is_empty() {
                        "No video response".into()
                    } else {
                        s
                    }
                }
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    format!("Error: {}", platform::redact_url(&err))
                }
                Err(e) => format!("Could not run ffprobe: {e}"),
            };
            let _ = tx.send(text);
        });
        self.probe = Some(Probe { rx });
        self.probe_text = "Testing camera…".into();
    }

    fn handle_probe(&mut self) {
        let Some(probe) = self.probe.take() else {
            return;
        };
        match probe.rx.try_recv() {
            Ok(text) => {
                self.probe_text = text;
                self.probe = None;
            }
            Err(TryRecvError::Empty) => {
                self.probe = Some(probe);
            }
            Err(TryRecvError::Disconnected) => {
                self.probe_text = "Test interrupted".into();
                self.probe = None;
            }
        }
    }

    fn maintain(&mut self, ctx: &egui::Context) {
        if self.needs_sync {
            self.sync_players();
            self.needs_sync = false;
        }

        if self.last_tunnel_check.elapsed() > Duration::from_secs(2) {
            self.last_tunnel_check = Instant::now();
            let changes = self.tunnels.check_and_restart(&self.cfg.cameras);
            for (id, old, new) in changes {
                platform::log_line(&format!("Tunnel {id} changed {old} -> {new}"));
                if let Some(cam) = self.cfg.camera(&id).cloned() {
                    let kind = self
                        .player_stream
                        .get(&id)
                        .copied()
                        .unwrap_or(StreamKind::Sub);
                    if let Some(player) = self.players.get(&id).cloned() {
                        let url = cam.url_for(Some(new), kind);
                        if let Ok(mut p) = player.lock() {
                            p.load(url);
                        }
                    }
                }
            }
        }

        let ids: Vec<String> = self.players.keys().cloned().collect();
        for id in ids {
            if let Some(player) = self.players.get(&id) {
                if let Ok(mut p) = player.lock() {
                    p.tick();
                }
            }
        }

        self.maintain_recordings();

        if self.storage_prune_now || self.last_storage_check.elapsed() > Duration::from_secs(10) {
            self.storage_prune_now = false;
            self.last_storage_check = Instant::now();
            self.refresh_storage(true);
        }

        if self.last_disk_check.elapsed() > Duration::from_secs(5) {
            self.last_disk_check = Instant::now();
            self.disk_free = platform::disk_free_mb(Path::new(&self.cfg.storage.dir)).unwrap_or(0);
            self.mem_avail = platform::mem_available_mb().unwrap_or(0);
            if self.any_recording() && self.disk_free < self.cfg.storage.min_free_mb {
                self.refresh_storage(true);
                self.disk_free =
                    platform::disk_free_mb(Path::new(&self.cfg.storage.dir)).unwrap_or(0);
                if self.disk_free < self.cfg.storage.min_free_mb {
                    self.stop_all_recordings();
                    self.toast(format!(
                        "Less than {} MB free: recordings stopped",
                        self.cfg.storage.min_free_mb
                    ));
                }
            }
            if self.mem_avail > 0 && self.mem_avail < 100 && self.cfg.app.render_fps > 5 {
                self.cfg.app.render_fps = 5;
                self.cfg.app.profile = "eco".into();
                self.dirty = true;
                self.toast("Low RAM: Eco mode enabled (5 fps)");
            }
        }

        if self.dirty && self.last_save.elapsed() > Duration::from_secs(2) {
            self.last_save = Instant::now();
            if let Err(e) = self.cfg.save() {
                self.toast(e);
            } else {
                self.dirty = false;
            }
        }

        if self
            .toast
            .as_ref()
            .map(|(_, t)| t.elapsed() > Duration::from_secs(8))
            .unwrap_or(false)
        {
            self.toast = None;
        }

        if let Some(full) = &self.fullscreen {
            if self.cfg.camera(full).map(|c| !c.enabled).unwrap_or(true) {
                self.fullscreen = None;
                self.zoom.reset();
                self.needs_sync = true;
            }
        }
        self.clamp_page();
        self.handle_probe();

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.fullscreen.is_some() {
            self.set_fullscreen(None);
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .exact_height(52.0)
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("CamView").size(22.0).strong().color(ACCENT));
                    ui.label(RichText::new("security").size(11.0).color(MUTED));
                    ui.separator();

                    let mut profile = self.cfg.app.profile.clone();
                    egui::ComboBox::from_id_salt("top_profile")
                        .selected_text(match profile.as_str() {
                            "eco" => "Eco",
                            _ => "Balanced",
                        })
                        .width(115.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut profile, "eco".into(), "Eco (lower RAM/CPU)");
                            ui.selectable_value(&mut profile, "balanced".into(), "Balanced");
                        });
                    if profile != self.cfg.app.profile {
                        self.set_profile(&profile);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Settings").clicked() {
                            self.show_settings = true;
                        }
                        if ui.button("Cameras").clicked() {
                            self.show_cameras = true;
                        }
                        if self.any_recording() {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Stop all").color(Color32::BLACK),
                                    )
                                    .fill(RED),
                                )
                                .clicked()
                            {
                                self.apply_action(UiAction::StopAll);
                            }
                        } else if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Record all").color(Color32::BLACK),
                                )
                                .fill(GREEN),
                            )
                            .clicked()
                        {
                            self.apply_action(UiAction::RecordAll);
                        }
                        let live = self
                            .players
                            .values()
                            .filter(|p| p.lock().map(|p| p.state.is_live()).unwrap_or(false))
                            .count();
                        ui.label(
                            RichText::new(format!("{live} live"))
                                .size(12.0)
                                .color(if live > 0 { GREEN } else { MUTED }),
                        );
                    });
                });
            });
    }

    fn bottom_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom")
            .exact_height(46.0)
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                let selected = self.selected.clone();
                let info = selected
                    .as_deref()
                    .map(|id| self.player_info(id))
                    .unwrap_or_default();
                let mut action = None;

                ui.horizontal_centered(|ui| {
                    if let Some(id) = &selected {
                        let name = self
                            .cfg
                            .camera(id)
                            .map(|c| c.name.clone())
                            .unwrap_or_else(|| "Camera".into());
                        ui.label(RichText::new(name).strong());
                        ui.separator();
                        if ui.button("Snapshot").clicked() {
                            action = Some(UiAction::Snapshot(id.clone()));
                        }
                        if ui
                            .button(if info.audio { "Mute" } else { "Listen" })
                            .clicked()
                        {
                            action = Some(UiAction::ToggleAudio(id.clone()));
                        }
                        if ui
                            .add(
                                egui::Button::new(if info.record_desired {
                                    "Stop recording"
                                } else {
                                    "Record"
                                })
                                .fill(if info.record_desired {
                                    RED
                                } else {
                                    CARD_HOVER
                                }),
                            )
                            .clicked()
                        {
                            action = Some(UiAction::ToggleRecord(id.clone()));
                        }
                        if info.recording {
                            ui.label(
                                RichText::new(format!("REC {}", format_duration(info.record_secs)))
                                    .color(RED)
                                    .strong(),
                            );
                        }
                        if ui
                            .button(if self.fullscreen.is_some() {
                                "Exit"
                            } else {
                                "Fullscreen"
                            })
                            .clicked()
                        {
                            action = Some(UiAction::Fullscreen(if self.fullscreen.is_some() {
                                None
                            } else {
                                Some(id.clone())
                            }));
                        }
                        if self.fullscreen.is_some() {
                            ui.separator();
                            if ui.small_button("−").clicked() {
                                self.adjust_zoom(1.0 / 1.25);
                            }
                            ui.label(
                                RichText::new(format!("{:.0}%", self.zoom.percent()))
                                    .size(11.0)
                                    .color(MUTED),
                            );
                            if ui.small_button("+").clicked() {
                                self.adjust_zoom(1.25);
                            }
                            if ui.small_button("Reset").clicked() {
                                self.zoom.reset();
                            }
                        }
                    } else {
                        ui.label(RichText::new("Select a camera").color(MUTED));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let usage = platform::format_mb(self.record_usage_bytes / (1024 * 1024));
                        let limit = if self.cfg.recording.max_storage_mb == 0 {
                            "unlimited".to_string()
                        } else {
                            platform::format_mb(self.cfg.recording.max_storage_mb)
                        };
                        ui.label(
                            RichText::new(format!("Clips {usage} / {limit}"))
                                .size(11.0)
                                .color(MUTED),
                        );
                        let pages = self.pages();
                        if pages > 1 {
                            if ui
                                .add_enabled(self.page + 1 < pages, egui::Button::new("›"))
                                .clicked()
                            {
                                self.page += 1;
                                self.needs_sync = true;
                            }
                            ui.label(
                                RichText::new(format!("Page {}/{}", self.page + 1, pages))
                                    .size(11.0)
                                    .color(MUTED),
                            );
                            if ui
                                .add_enabled(self.page > 0, egui::Button::new("‹"))
                                .clicked()
                            {
                                self.page -= 1;
                                self.needs_sync = true;
                            }
                        }
                        ui.label(
                            RichText::new(format!(
                                "RAM free {}  ·  Disk free {}",
                                platform::format_mb(self.mem_avail),
                                platform::format_mb(self.disk_free)
                            ))
                            .size(11.0)
                            .color(MUTED),
                        );
                    });
                });
                if let Some(action) = action {
                    self.apply_action(action);
                }
            });
    }

    fn draw_add_tile(&self, ui: &mut egui::Ui, rect: Rect) -> bool {
        let response = ui.allocate_rect(rect, Sense::click());
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(8), CARD);
        painter.rect_stroke(
            rect,
            CornerRadius::same(8),
            Stroke::new(1.0, Color32::from_rgb(55, 62, 76)),
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "+ Add camera",
            FontId::proportional(18.0),
            MUTED,
        );
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
    }

    fn handle_zoom_input(
        &mut self,
        ui: &egui::Ui,
        video: Rect,
        content: Rect,
        response: &egui::Response,
    ) {
        if !ui.ctx().wants_keyboard_input() {
            let (zoom_in, zoom_out, reset) = ui.ctx().input(|i| {
                (
                    i.modifiers.command
                        && (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)),
                    i.modifiers.command && i.key_pressed(egui::Key::Minus),
                    i.modifiers.command && i.key_pressed(egui::Key::Num0),
                )
            });
            if reset {
                self.zoom.reset();
            }
            if zoom_in {
                self.zoom.zoom_center(video, content, 1.25);
            }
            if zoom_out {
                self.zoom.zoom_center(video, content, 1.0 / 1.25);
            }
        }

        let (multi, zoom_delta, scroll) = ui
            .ctx()
            .input(|i| (i.multi_touch(), i.zoom_delta(), i.raw_scroll_delta));

        if let Some(multi) = multi {
            if video.contains(multi.center_pos) {
                self.zoom
                    .zoom_at(video, content, multi.center_pos, multi.zoom_delta);
                self.zoom.pan_screen(content, multi.translation_delta);
            }
            return;
        }

        if self.zoom.is_zoomed() && response.dragged_by(PointerButton::Primary) {
            self.zoom.pan_screen(content, response.drag_delta());
        }

        let Some(pos) = response.hover_pos() else {
            return;
        };
        if !video.contains(pos) {
            return;
        }

        // Ctrl+wheel and pinch-zoom events arrive as zoom_delta. A plain wheel
        // or two-finger touchpad scroll zooms as well, because fullscreen video
        // has nothing else to scroll.
        let factor = if (zoom_delta - 1.0).abs() > 0.0001 {
            zoom_delta
        } else if scroll.y.abs() > 0.001 {
            (scroll.y * 0.0025).exp()
        } else {
            1.0
        };
        if (factor - 1.0).abs() > 0.0001 {
            self.zoom.zoom_at(video, content, pos, factor);
        }
    }

    fn draw_tile(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        cam: &Camera,
        fullscreen: bool,
    ) -> Option<UiAction> {
        let sense = if fullscreen {
            Sense::click_and_drag()
        } else {
            Sense::click()
        };
        let response = ui.allocate_rect(rect, sense);
        let info = self.player_info(&cam.id);
        let selected = self.selected.as_deref() == Some(cam.id.as_str());
        let aspect = if info.width > 0 && info.height > 0 {
            info.aspect
        } else {
            16.0 / 9.0
        };
        let content = zoom::fit_content(rect, aspect);

        if fullscreen {
            self.zoom_video_rect = rect;
            self.zoom_content_rect = content;
            self.handle_zoom_input(ui, rect, content, &response);
        }
        let view = if fullscreen {
            self.zoom
        } else {
            ZoomView::default()
        };
        let (uv_offset, uv_scale) = view.uv_transform();
        let high_res = fullscreen && view.is_zoomed();

        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(8), CARD);

        if let (Some(player), Some(tile)) = (self.players.get(&cam.id), self.gl_tiles.get(&cam.id))
        {
            let player = player.clone();
            let tile = tile.clone();
            let uv_offset = [uv_offset.x, uv_offset.y];
            let uv_scale = [uv_scale.x, uv_scale.y];
            let callback = eframe::egui_glow::CallbackFn::new(move |info, painter| {
                let vp = info.viewport_in_pixels();
                let gl = painter.gl();
                if let (Ok(p), Ok(mut t)) = (player.lock(), tile.lock()) {
                    t.draw(
                        gl,
                        &p,
                        vp.left_px,
                        vp.from_bottom_px,
                        vp.width_px,
                        vp.height_px,
                        uv_offset,
                        uv_scale,
                        high_res,
                    );
                }
            });
            painter.add(egui::PaintCallback {
                rect,
                callback: Arc::new(callback),
            });
        }

        if !info.state.is_live() {
            painter.rect_filled(
                rect,
                CornerRadius::same(8),
                Color32::from_rgba_unmultiplied(8, 10, 14, 200),
            );
            let color = state_color(&info.state);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                info.state.label(),
                FontId::proportional(if fullscreen { 22.0 } else { 16.0 }),
                color,
            );
            if !info.state.is_live() && !matches!(info.state, PlayerState::Connecting) {
                painter.text(
                    rect.center() + Vec2::new(0.0, 26.0),
                    Align2::CENTER_CENTER,
                    "Camera reconnects automatically",
                    FontId::proportional(11.0),
                    MUTED,
                );
            }
        }

        let top_h = if fullscreen { 48.0 } else { 42.0 };
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(rect.width(), top_h)),
            CornerRadius::same(8),
            Color32::from_black_alpha(145),
        );
        painter.circle_filled(
            rect.min + Vec2::new(16.0, 28.0),
            4.0,
            state_color(&info.state),
        );
        painter.text(
            rect.min + Vec2::new(26.0, 8.0),
            Align2::LEFT_TOP,
            &cam.name,
            FontId::proportional(if fullscreen { 18.0 } else { 15.0 }),
            Color32::WHITE,
        );
        painter.text(
            rect.min + Vec2::new(26.0, 24.0),
            Align2::LEFT_TOP,
            if info.state.is_live() {
                format!("{}x{} · {}", info.width, info.height, info.hwdec)
            } else {
                info.state.label()
            },
            FontId::proportional(11.0),
            MUTED,
        );
        if info.recording {
            let text = format!("REC {}", format_duration(info.record_secs));
            painter.rect_filled(
                Rect::from_min_size(
                    rect.right_top() + Vec2::new(-112.0, 9.0),
                    Vec2::new(102.0, 22.0),
                ),
                CornerRadius::same(5),
                Color32::from_rgba_unmultiplied(120, 20, 20, 210),
            );
            painter.text(
                rect.right_top() + Vec2::new(-61.0, 20.0),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
        if info.audio {
            painter.text(
                rect.right_top() + Vec2::new(-14.0, 40.0),
                Align2::RIGHT_TOP,
                "AUDIO",
                FontId::proportional(10.0),
                GREEN,
            );
        }

        let host_text = format!("{}:{}", cam.host, cam.rtsp_port);
        let host_rect = Rect::from_min_size(
            rect.left_bottom() + Vec2::new(8.0, -24.0),
            Vec2::new(150.0, 18.0),
        );
        painter.rect_filled(
            host_rect,
            CornerRadius::same(4),
            Color32::from_black_alpha(135),
        );
        painter.text(
            host_rect.left_center() + Vec2::new(7.0, 0.0),
            Align2::LEFT_CENTER,
            host_text,
            FontId::proportional(10.0),
            MUTED,
        );

        if selected {
            painter.rect_stroke(
                rect,
                CornerRadius::same(8),
                Stroke::new(2.0, ACCENT),
                StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_stroke(
                rect,
                CornerRadius::same(8),
                Stroke::new(1.0, Color32::from_rgb(80, 90, 105)),
                StrokeKind::Inside,
            );
        }

        let mut action = None;
        response.context_menu(|ui| {
            if ui
                .button(if fullscreen {
                    "Exit fullscreen"
                } else {
                    "Fullscreen"
                })
                .clicked()
            {
                action = Some(UiAction::Fullscreen(if fullscreen {
                    None
                } else {
                    Some(cam.id.clone())
                }));
                ui.close();
            }
            if ui.button("Snapshot").clicked() {
                action = Some(UiAction::Snapshot(cam.id.clone()));
                ui.close();
            }
            if ui
                .button(if info.record_desired {
                    "Stop recording"
                } else {
                    "Record"
                })
                .clicked()
            {
                action = Some(UiAction::ToggleRecord(cam.id.clone()));
                ui.close();
            }
            if ui
                .button(if info.audio { "Mute" } else { "Listen" })
                .clicked()
            {
                action = Some(UiAction::ToggleAudio(cam.id.clone()));
                ui.close();
            }
            ui.separator();
            if ui.button("Edit camera").clicked() {
                action = Some(UiAction::EditCamera(cam.id.clone()));
                ui.close();
            }
        });
        if action.is_some() {
            return action;
        }

        if response.double_clicked() {
            if fullscreen && self.zoom.is_zoomed() {
                self.zoom.reset();
                return None;
            }
            return Some(UiAction::Fullscreen(if fullscreen {
                None
            } else {
                Some(cam.id.clone())
            }));
        }
        if response.clicked() {
            return Some(UiAction::Select(cam.id.clone()));
        }
        None
    }

    fn draw_grid(&mut self, ui: &mut egui::Ui) {
        let cams = self.page_cameras();
        let per = self.cfg.app.max_live_tiles.max(1);
        let cols = if per <= 1 {
            1
        } else if per <= 4 {
            2
        } else {
            3
        };
        let rows = (per + cols - 1) / cols;
        let gap = 8.0;
        let rect = ui.available_rect_before_wrap();
        let tw = ((rect.width() - gap * (cols as f32 - 1.0)) / cols as f32).max(1.0);
        let th = ((rect.height() - gap * (rows as f32 - 1.0)) / rows as f32).max(1.0);

        let mut actions = Vec::new();
        for (idx, cam) in cams.iter().enumerate() {
            let row = idx / cols;
            let col = idx % cols;
            let pos = rect.min + Vec2::new(col as f32 * (tw + gap), row as f32 * (th + gap));
            let tile = Rect::from_min_size(pos, Vec2::new(tw, th));
            if let Some(action) = self.draw_tile(ui, tile, cam, false) {
                actions.push(action);
            }
        }

        if cams.len() < per {
            let idx = cams.len();
            let row = idx / cols;
            let col = idx % cols;
            let pos = rect.min + Vec2::new(col as f32 * (tw + gap), row as f32 * (th + gap));
            let tile = Rect::from_min_size(pos, Vec2::new(tw, th));
            if self.draw_add_tile(ui, tile) {
                actions.push(UiAction::AddCamera);
            }
        }

        for action in actions {
            self.apply_action(action);
        }
    }

    fn draw_fullscreen(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(cam) = self.cfg.camera(id).cloned() else {
            self.fullscreen = None;
            return;
        };
        let rect = ui.available_rect_before_wrap();
        if let Some(action) = self.draw_tile(ui, rect, &cam, true) {
            self.apply_action(action);
        }
    }

    fn cameras_window(&mut self, ctx: &egui::Context) {
        if !self.show_cameras {
            return;
        }
        let cams = self.cfg.cameras.clone();
        let probe_text = self.probe_text.clone();
        let mut open = self.show_cameras;
        let mut actions = Vec::new();
        egui::Window::new("Cameras")
            .open(&mut open)
            .default_width(760.0)
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("+ Add camera").clicked() {
                        actions.push(UiAction::AddCamera);
                    }
                    if self.probe.is_some() {
                        ui.spinner();
                    }
                    if !probe_text.is_empty() {
                        ui.label(RichText::new(&probe_text).size(11.0).color(MUTED));
                    }
                });
                ui.separator();

                if let Some(del) = &self.delete_confirm {
                    let name = cams
                        .iter()
                        .find(|c| &c.id == del)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| del.clone());
                    ui.horizontal(|ui| {
                        ui.colored_label(RED, format!("Delete “{name}”?"));
                        if ui.button("Yes, delete").clicked() {
                            actions.push(UiAction::ConfirmDelete(del.clone()));
                        }
                        if ui.button("Cancel").clicked() {
                            actions.push(UiAction::CancelDelete);
                        }
                    });
                    ui.separator();
                }

                egui::ScrollArea::vertical()
                    .max_height(430.0)
                    .show(ui, |ui| {
                        for cam in &cams {
                            let info = self.player_info(&cam.id);
                            ui.horizontal(|ui| {
                                let mut enabled = cam.enabled;
                                if ui.checkbox(&mut enabled, "").changed() {
                                    actions.push(UiAction::ToggleEnabled(cam.id.clone(), enabled));
                                }
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(&cam.name).strong().size(15.0));
                                        ui.colored_label(
                                            state_color(&info.state),
                                            info.state.label(),
                                        );
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "{}  ·  {}",
                                            cam.host_label(),
                                            if cam.tls { "RTSPS" } else { "RTSP" }
                                        ))
                                        .size(11.0)
                                        .color(MUTED),
                                    );
                                    if let Some(err) = self.tunnels.error_for(&cam.id) {
                                        ui.label(RichText::new(err).size(10.0).color(RED));
                                    }
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("Delete").clicked() {
                                            actions.push(UiAction::DeleteCamera(cam.id.clone()));
                                        }
                                        if ui.button("Edit").clicked() {
                                            actions.push(UiAction::EditCamera(cam.id.clone()));
                                        }
                                        if ui.button("Test").clicked() {
                                            actions.push(UiAction::TestCamera(cam.id.clone()));
                                        }
                                    },
                                );
                            });
                            ui.separator();
                        }
                        if cams.is_empty() {
                            ui.label(
                                RichText::new(
                                    "No cameras yet. Use “+ Add camera” or import an M3U playlist.",
                                )
                                .color(MUTED),
                            );
                        }
                    });
            });
        self.show_cameras = open;
        for action in actions {
            self.apply_action(action);
        }
    }

    fn camera_form_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.camera_form.take() else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(if form.editing {
            "Edit camera"
        } else {
            "Add camera"
        })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(430.0)
        .show(ctx, |ui| {
            egui::Grid::new("cam_form_grid")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    ui.text_edit_singleline(&mut form.name);
                    ui.end_row();

                    ui.label("IP / host");
                    ui.text_edit_singleline(&mut form.host);
                    ui.end_row();

                    ui.label("RTSP port");
                    ui.add(egui::TextEdit::singleline(&mut form.port).desired_width(80.0));
                    ui.end_row();

                    ui.label("Username");
                    ui.text_edit_singleline(&mut form.user);
                    ui.end_row();

                    ui.label("Password");
                    ui.add(egui::TextEdit::singleline(&mut form.password).password(true));
                    ui.end_row();

                    ui.label("Main path");
                    ui.text_edit_singleline(&mut form.main_path);
                    ui.end_row();

                    ui.label("Sub path");
                    ui.text_edit_singleline(&mut form.sub_path);
                    ui.end_row();

                    ui.label("TLS");
                    ui.checkbox(&mut form.tls, "RTSPS (local tunnel)");
                    ui.end_row();

                    ui.label("Enabled");
                    ui.checkbox(&mut form.enabled, "Show in the viewer");
                    ui.end_row();
                });
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "The TLS tunnel runs on 127.0.0.1 and is independent of other players. \
                     CamView uses one connection per camera.",
                )
                .size(11.0)
                .color(MUTED),
            );
            if let Some(err) = &form.error {
                ui.add_space(4.0);
                ui.colored_label(RED, err);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Save").fill(ACCENT)).clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

        if save {
            match self.commit_camera(&form) {
                Ok(()) => {
                    self.camera_form = None;
                }
                Err(e) => {
                    form.error = Some(e);
                    self.camera_form = Some(form);
                }
            }
        } else if !cancel && open {
            self.camera_form = Some(form);
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = self.show_settings;
        let mut profile = self.cfg.app.profile.clone();
        let mut fps = self.cfg.app.render_fps;
        let mut max_tiles = self.cfg.app.max_live_tiles;
        let mut storage = self.cfg.storage.dir.clone();
        let mut min_free = self.cfg.storage.min_free_mb;
        let mut max_gb = self.cfg.recording.max_storage_mb as f32 / 1024.0;
        let mut autostart = self.cfg.app.autostart;
        let mut open_storage = false;
        let mut changed = false;

        egui::Window::new("Settings")
            .open(&mut open)
            .default_width(520.0)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.heading("Performance");
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Profile");
                        egui::ComboBox::from_id_salt("settings_profile")
                            .selected_text(match profile.as_str() {
                                "eco" => "Eco",
                                _ => "Balanced",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut profile,
                                    "eco".into(),
                                    "Eco (5 fps, lower power)",
                                );
                                ui.selectable_value(
                                    &mut profile,
                                    "balanced".into(),
                                    "Balanced (15 fps)",
                                );
                            });
                        ui.end_row();

                        ui.label("Render FPS");
                        ui.add(egui::Slider::new(&mut fps, 5..=20).suffix(" fps"));
                        ui.end_row();

                        ui.label("Max live cameras");
                        ui.add(egui::Slider::new(&mut max_tiles, 1..=9));
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.heading("Recording");
                egui::Grid::new("storage_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Folder");
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut storage).desired_width(300.0));
                            if ui.button("Open").clicked() {
                                open_storage = true;
                            }
                        });
                        ui.end_row();

                        ui.label("Maximum recording storage");
                        ui.add(
                            egui::Slider::new(&mut max_gb, 0.0..=20.0)
                                .step_by(0.5)
                                .suffix(" GB"),
                        );
                        ui.end_row();

                        ui.label("Recordings use");
                        ui.label(
                            RichText::new(platform::format_mb(
                                self.record_usage_bytes / (1024 * 1024),
                            ))
                            .color(MUTED),
                        );
                        ui.end_row();

                        ui.label("Minimum free space");
                        ui.add(egui::Slider::new(&mut min_free, 100..=5000).suffix(" MB"));
                        ui.end_row();
                    });
                ui.label(
                    RichText::new(
                        "Recording is a direct stream copy (no re-encoding). \
                         Oldest clips are deleted automatically when the limit is reached; \
                         0 GB means unlimited. It also stops if free disk space falls below the minimum.",
                    )
                    .size(11.0)
                    .color(MUTED),
                );

                ui.add_space(8.0);
                ui.heading("System");
                ui.checkbox(&mut autostart, "Launch CamView at login");
                ui.label(
                    RichText::new("Language: English · Dark theme")
                        .size(11.0)
                        .color(MUTED),
                );
                ui.horizontal(|ui| {
                    if ui.button("Open config folder").clicked() {
                        platform::open_path(&platform::config_dir());
                    }
                    if ui.button("Open snapshots folder").clicked() {
                        platform::open_path(&platform::snapshots_dir());
                    }
                });
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "CamView v{}  ·  libmpv  ·  VA-API  ·  one connection per camera",
                        env!("CARGO_PKG_VERSION")
                    ))
                    .size(11.0)
                    .color(MUTED),
                );
            });

        if profile != self.cfg.app.profile {
            self.cfg.app.profile = profile;
            self.cfg.app.render_fps = if self.cfg.app.profile == "eco" { 5 } else { 15 };
            changed = true;
            fps = self.cfg.app.render_fps;
        }
        if fps != self.cfg.app.render_fps {
            self.cfg.app.render_fps = fps;
            changed = true;
        }
        if max_tiles != self.cfg.app.max_live_tiles {
            self.cfg.app.max_live_tiles = max_tiles;
            self.needs_sync = true;
            changed = true;
        }
        if storage != self.cfg.storage.dir {
            self.cfg.storage.dir = storage;
            let _ = platform::ensure_dir(Path::new(&self.cfg.storage.dir));
            self.storage_prune_now = true;
            changed = true;
        }
        if min_free != self.cfg.storage.min_free_mb {
            self.cfg.storage.min_free_mb = min_free;
            changed = true;
        }
        let max_storage_mb = (max_gb.max(0.0) * 1024.0).round() as u64;
        if max_storage_mb != self.cfg.recording.max_storage_mb {
            self.cfg.recording.max_storage_mb = max_storage_mb;
            self.storage_prune_now = true;
            changed = true;
        }
        if autostart != self.cfg.app.autostart {
            self.cfg.app.autostart = autostart;
            if let Err(e) = platform::set_autostart(autostart) {
                self.toast(e);
            }
            changed = true;
        }
        if open_storage {
            let dir = PathBuf::from(&self.cfg.storage.dir);
            let _ = platform::ensure_dir(&dir);
            platform::open_path(&dir);
        }
        if changed {
            self.dirty = true;
        }
        self.show_settings = open;
    }

    fn draw_toast(&self, ctx: &egui::Context) {
        let Some((msg, _)) = &self.toast else {
            return;
        };
        egui::Area::new("toast".into())
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 64.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(Color32::from_rgb(30, 35, 46))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(60, 70, 88)))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(14, 9))
                    .show(ui, |ui| {
                        ui.label(RichText::new(msg).size(13.0).color(TEXT));
                    });
            });
    }

    fn fatal_screen(&self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.label(RichText::new("CamView").size(30.0).strong().color(ACCENT));
                    ui.add_space(12.0);
                    if let Some(e) = &self.fatal {
                        ui.colored_label(RED, e);
                    }
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("See ~/.local/state/camview/camview.log for details.")
                            .color(MUTED),
                    );
                });
            });
    }
}

impl eframe::App for CamViewApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if crate::shutdown_requested() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.fatal.is_some() {
            self.fatal_screen(ctx);
            return;
        }

        self.maintain(ctx);
        self.top_bar(ctx);
        self.bottom_bar(ctx);

        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG).inner_margin(Margin::same(8)))
            .show(ctx, |ui| {
                if let Some(id) = self.fullscreen.clone() {
                    self.draw_fullscreen(ui, &id);
                } else {
                    self.draw_grid(ui);
                }
            });

        self.cameras_window(ctx);
        self.settings_window(ctx);
        self.camera_form_window(ctx);
        self.draw_toast(ctx);

        let mut ms = 1000 / self.cfg.app.render_fps.clamp(5, 20);
        let reconnecting = self.players.values().any(|p| {
            p.lock()
                .map(|p| {
                    matches!(
                        p.state,
                        PlayerState::Connecting | PlayerState::Reconnecting { .. }
                    )
                })
                .unwrap_or(false)
        });
        if reconnecting {
            ms = ms.min(250);
        }
        ctx.request_repaint_after(Duration::from_millis(ms as u64));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_all_recordings();
        let _ = self.cfg.save();
        platform::log_line("CamView closed");
    }
}

fn state_color(state: &PlayerState) -> Color32 {
    match state {
        PlayerState::Live => GREEN,
        PlayerState::Connecting => YELLOW,
        PlayerState::Reconnecting { .. } => ORANGE,
        PlayerState::Error(_) => RED,
        PlayerState::Idle | PlayerState::Stopped => MUTED,
    }
}

fn format_duration(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}
