use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use libmpv2::Mpv;

use crate::platform;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerState {
    Idle,
    Connecting,
    Live,
    Reconnecting {
        seconds: u64,
    },
    #[allow(dead_code)]
    Error(String),
    Stopped,
}

impl PlayerState {
    pub fn label(&self) -> String {
        match self {
            PlayerState::Idle => "Stopped".into(),
            PlayerState::Connecting => "Connecting…".into(),
            PlayerState::Live => "Live".into(),
            PlayerState::Reconnecting { seconds } => format!("Reconnecting in {seconds}s"),
            PlayerState::Error(e) => format!("Error: {e}"),
            PlayerState::Stopped => "Stopped".into(),
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self, PlayerState::Live)
    }
}

struct GlLoader;

fn gl_proc(_ctx: &GlLoader, name: &str) -> *mut c_void {
    platform::gl_get_proc(name)
}

fn set_opts(init: &libmpv2::MpvInitializer, vo: &str) -> libmpv2::Result<()> {
    init.set_option("vo", vo.to_string())?;
    init.set_option("hwdec", "auto-safe".to_string())?;
    init.set_option("audio", "auto".to_string())?;
    init.set_option("volume", "0".to_string())?;
    init.set_option("cache", "yes".to_string())?;
    init.set_option("demuxer-max-bytes", "8MiB".to_string())?;
    init.set_option("demuxer-max-back-bytes", "1MiB".to_string())?;
    init.set_option("demuxer-readahead-secs", "1".to_string())?;
    init.set_option("demuxer-lavf-o", "rtsp_transport=tcp".to_string())?;
    init.set_option("rtsp-transport", "tcp".to_string())?;
    init.set_option("network-timeout", "5".to_string())?;
    init.set_option("vd-lavc-threads", "2".to_string())?;
    init.set_option("video-timing-offset", "0".to_string())?;
    init.set_option("osc", false)?;
    init.set_option("osd-level", "0".to_string())?;
    init.set_option("sub", "no".to_string())?;
    init.set_option("sub-auto", "no".to_string())?;
    init.set_option("audio-file-auto", "no".to_string())?;
    init.set_option("input-default-bindings", false)?;
    init.set_option("input-vo-keyboard", false)?;
    init.set_option("input-terminal", false)?;
    init.set_option("terminal", false)?;
    init.set_option("idle", true)?;
    init.set_option("keep-open", false)?;
    init.set_option("force-window", false)?;
    init.set_option("screenshot-format", "jpg".to_string())?;
    init.set_option("screenshot-jpeg-quality", "85".to_string())?;
    init.set_option("msg-level", "all=error".to_string())?;
    unsafe {
        let _ = libc::setlocale(libc::LC_NUMERIC, b"C\0".as_ptr() as *const _);
    }
    Ok(())
}

/// Creates a headless mpv, used by the self-test and camera probing.
pub fn create_headless() -> Result<Mpv, String> {
    Mpv::with_initializer(|init| set_opts(&init, "null")).map_err(|e| format!("mpv: {e}"))
}

pub struct Player {
    // The render context must be destroyed before mpv. Field order matters.
    render: Option<RenderContext<'static>>,
    mpv: Box<Mpv>,
    pub id: String,
    pub state: PlayerState,
    pub width: u32,
    pub height: u32,
    pub hwdec: String,
    pub audio: bool,
    recording: Option<PathBuf>,
    pub record_started: Option<Instant>,
    pub last_record_path: Option<PathBuf>,
    url: String,
    loaded_at: Instant,
    last_live: Instant,
    last_poll: Instant,
    retry_at: Option<Instant>,
    retries: u32,
    pub last_error: String,
}

unsafe impl Send for Player {}

impl Player {
    pub fn new(id: &str, x11_display: Option<usize>) -> Result<Self, String> {
        let mpv = Box::new(
            Mpv::with_initializer(|init| set_opts(&init, "libmpv"))
                .map_err(|e| format!("Could not create mpv: {e}"))?,
        );
        let _ = mpv.set_property("volume", 0.0f64);
        let mpv_ref: &'static Mpv = unsafe { &*(mpv.as_ref() as *const Mpv) };

        let mut params: Vec<RenderParam<GlLoader>> = vec![
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams {
                get_proc_address: gl_proc,
                ctx: GlLoader,
            }),
        ];
        if let Some(display) = x11_display {
            params.push(RenderParam::X11Display(display as *const c_void));
        }

        let render = mpv_ref
            .create_render_context(params)
            .map_err(|e| format!("Could not create video context: {e}"))?;

        let now = Instant::now();
        Ok(Self {
            render: Some(render),
            mpv,
            id: id.to_string(),
            state: PlayerState::Idle,
            width: 0,
            height: 0,
            hwdec: String::new(),
            audio: false,
            recording: None,
            record_started: None,
            last_record_path: None,
            url: String::new(),
            loaded_at: now,
            last_live: now,
            last_poll: now - Duration::from_secs(5),
            retry_at: None,
            retries: 0,
            last_error: String::new(),
        })
    }

    pub fn load(&mut self, url: String) {
        let was_recording = self.recording.is_some();
        if was_recording {
            self.stop_recording();
        }
        self.url = url;
        self.state = PlayerState::Connecting;
        self.loaded_at = Instant::now();
        self.retry_at = None;
        self.last_error.clear();
        let u = self.url.clone();
        if let Err(e) = self.mpv.command("loadfile", &[&u, "replace"]) {
            self.last_error = format!("{e}");
            self.schedule_retry();
        } else {
            platform::log_line(&format!(
                "Loading {} -> {}",
                self.id,
                platform::redact_url(&u)
            ));
        }
    }

    pub fn set_audio(&mut self, enabled: bool) {
        // volume=0 mutes the output without disabling the track: stream-record still
        // includes audio in the recording.
        let volume = if enabled { 100.0 } else { 0.0 };
        if self.mpv.set_property("volume", volume).is_err() {
            let value = if enabled { "auto" } else { "no" };
            let _ = self.mpv.set_property("aid", value.to_string());
        }
        self.audio = enabled;
    }

    pub fn tick(&mut self) {
        if self.last_poll.elapsed() < Duration::from_millis(300) {
            return;
        }
        self.last_poll = Instant::now();

        if matches!(self.state, PlayerState::Idle | PlayerState::Stopped) {
            return;
        }

        let w: i64 = self.mpv.get_property("video-params/w").unwrap_or(0);
        let h: i64 = self.mpv.get_property("video-params/h").unwrap_or(0);
        let core_idle: bool = self.mpv.get_property("core-idle").unwrap_or(true);
        self.width = w.max(0) as u32;
        self.height = h.max(0) as u32;
        if let Ok(hw) = self.mpv.get_property::<String>("hwdec-current") {
            self.hwdec = hw;
        }

        let has_video = self.width > 0 && self.height > 0;
        if has_video && !core_idle {
            if !matches!(self.state, PlayerState::Live) {
                platform::log_line(&format!(
                    "{} live: {}x{} hwdec={}",
                    self.id, self.width, self.height, self.hwdec
                ));
            }
            self.state = PlayerState::Live;
            self.last_live = Instant::now();
            self.retries = 0;
            return;
        }

        match &self.state {
            PlayerState::Connecting => {
                if self.loaded_at.elapsed() > Duration::from_secs(10) {
                    self.last_error = "No camera signal".into();
                    self.schedule_retry();
                }
            }
            PlayerState::Live => {
                if self.last_live.elapsed() > Duration::from_secs(3) {
                    self.last_error = "Signal lost".into();
                    self.schedule_retry();
                }
            }
            PlayerState::Reconnecting { .. } => {
                if self.retry_at.map(|t| t <= Instant::now()).unwrap_or(false) {
                    let url = self.url.clone();
                    self.state = PlayerState::Connecting;
                    self.loaded_at = Instant::now();
                    self.retry_at = None;
                    if let Err(e) = self.mpv.command("loadfile", &[&url, "replace"]) {
                        self.last_error = format!("{e}");
                        self.schedule_retry();
                    }
                }
            }
            _ => {}
        }
    }

    fn schedule_retry(&mut self) {
        self.retries = (self.retries + 1).min(5);
        let seconds = match self.retries {
            1 => 2,
            2 => 4,
            3 => 8,
            4 => 15,
            _ => 30,
        };
        self.state = PlayerState::Reconnecting { seconds };
        self.retry_at = Some(Instant::now() + Duration::from_secs(seconds));
        platform::log_line(&format!(
            "{}: {} (retry in {seconds}s)",
            self.id, self.last_error
        ));
    }

    pub fn render_to_fbo(&self, fbo: i32, width: i32, height: i32) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        if let Some(render) = &self.render {
            let _ = render.render::<GlLoader>(fbo, width.max(1), height.max(1), true);
        }
    }

    pub fn snapshot(&self, camera_name: &str) -> Result<PathBuf, String> {
        let dir = platform::snapshots_dir();
        platform::ensure_dir(&dir)?;
        let file = dir.join(format!(
            "{}_{}.jpg",
            platform::sanitize_id(camera_name),
            platform::date_time_file()
        ));
        let path_str = file.to_string_lossy().into_owned();
        self.mpv
            .command("screenshot-to-file", &[&path_str, "video"])
            .map_err(|e| format!("Could not take snapshot: {e}"))?;
        Ok(file)
    }

    pub fn start_recording(
        &mut self,
        storage_dir: &Path,
        camera_slug: &str,
    ) -> Result<PathBuf, String> {
        if self.recording.is_some() {
            return Err("This camera is already recording".into());
        }
        let dir = storage_dir
            .join(platform::sanitize_id(camera_slug))
            .join(platform::date_dir());
        platform::ensure_dir(&dir)?;
        let file = dir.join(format!("{}.mkv", platform::time_file()));
        let path_str = file.to_string_lossy().into_owned();
        self.mpv
            .set_property("stream-record", path_str)
            .map_err(|e| format!("Could not start recording: {e}"))?;
        self.recording = Some(file.clone());
        self.record_started = Some(Instant::now());
        self.last_record_path = Some(file.clone());
        platform::log_line(&format!("{}: recording to {}", self.id, file.display()));
        Ok(file)
    }

    pub fn stop_recording(&mut self) -> Option<PathBuf> {
        if self.recording.is_none() {
            return None;
        }
        let _ = self.mpv.set_property("stream-record", String::new());
        platform::log_line(&format!("{}: recording stopped", self.id));
        self.record_started = None;
        self.recording.take()
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    pub fn stop(&mut self) {
        self.stop_recording();
        let _ = self.mpv.command("stop", &[]);
        self.state = PlayerState::Stopped;
        self.width = 0;
        self.height = 0;
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop_record();
        self.render.take();
    }
}

impl Player {
    fn stop_record(&mut self) {
        if self.recording.is_some() {
            let _ = self.mpv.set_property("stream-record", String::new());
            self.recording = None;
        }
    }
}
