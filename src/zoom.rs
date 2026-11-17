use eframe::egui::{Pos2, Rect, Vec2};

pub const MIN_ZOOM: f32 = 1.0;
pub const MAX_ZOOM: f32 = 8.0;
const EPSILON: f32 = 0.001;

/// Fullscreen video zoom. `center` is the visible point of the video in UV
/// coordinates: (0, 0) is the bottom-left and (1, 1) the top-right.
#[derive(Clone, Copy, Debug)]
pub struct ZoomView {
    pub zoom: f32,
    center: Vec2,
}

impl Default for ZoomView {
    fn default() -> Self {
        Self {
            zoom: MIN_ZOOM,
            center: Vec2::splat(0.5),
        }
    }
}

impl ZoomView {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn is_zoomed(&self) -> bool {
        self.zoom > MIN_ZOOM + EPSILON
    }

    pub fn percent(&self) -> f32 {
        self.zoom * 100.0
    }

    /// Zoom around a focal point in the video rectangle.
    pub fn zoom_at(&mut self, video: Rect, content: Rect, focal: Pos2, factor: f32) {
        let old = self.zoom;
        let new = (old * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (new - old).abs() < EPSILON {
            return;
        }

        let c = content.size();
        if c.x <= 0.0 || c.y <= 0.0 {
            return;
        }
        let from_center = focal - video.center();
        let delta = 1.0 / old - 1.0 / new;
        self.center.x += from_center.x / c.x * delta;
        // Screen y grows downwards; UV v grows upwards.
        self.center.y -= from_center.y / c.y * delta;
        self.zoom = new;
        self.clamp();
    }

    pub fn zoom_center(&mut self, video: Rect, content: Rect, factor: f32) {
        self.zoom_at(video, content, video.center(), factor);
    }

    /// Pan by a screen-space drag delta. Positive x moves the image right and
    /// positive y moves it down, as a direct-manipulation gesture.
    pub fn pan_screen(&mut self, content: Rect, delta: Vec2) {
        if !self.is_zoomed() {
            return;
        }
        let c = content.size();
        if c.x <= 0.0 || c.y <= 0.0 {
            return;
        }
        self.center.x -= delta.x / (c.x * self.zoom);
        self.center.y += delta.y / (c.y * self.zoom);
        self.clamp();
    }

    fn clamp(&mut self) {
        if self.zoom <= MIN_ZOOM + EPSILON {
            self.zoom = MIN_ZOOM;
            self.center = Vec2::splat(0.5);
            return;
        }
        let half = 0.5 / self.zoom;
        self.center.x = self.center.x.clamp(half, 1.0 - half);
        self.center.y = self.center.y.clamp(half, 1.0 - half);
    }

    /// UV offset and scale for the fragment shader.
    pub fn uv_transform(&self) -> (Vec2, Vec2) {
        let scale = 1.0 / self.zoom;
        let half = 0.5 * scale;
        (
            Vec2::new(self.center.x - half, self.center.y - half),
            Vec2::splat(scale),
        )
    }

    #[cfg(test)]
    fn uv_at_screen(&self, video: Rect, content: Rect, pos: Pos2) -> Vec2 {
        let d = pos - video.center();
        let c = content.size();
        Vec2::new(
            self.center.x + d.x / (c.x * self.zoom),
            self.center.y - d.y / (c.y * self.zoom),
        )
    }
}

/// Largest 16:9-like rectangle that fits inside `container`.
pub fn fit_content(container: Rect, aspect: f32) -> Rect {
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        16.0 / 9.0
    };
    let container_aspect = container.width() / container.height().max(1.0);
    let size = if container_aspect > aspect {
        Vec2::new(container.height() * aspect, container.height())
    } else {
        Vec2::new(container.width(), container.width() / aspect)
    };
    Rect::from_center_size(container.center(), size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(1280.0, 720.0))
    }

    #[test]
    fn fit_content_letterboxes_a_tall_container() {
        let container = Rect::from_min_size(Pos2::ZERO, Vec2::new(720.0, 720.0));
        let content = fit_content(container, 16.0 / 9.0);
        assert!((content.width() - 720.0).abs() < 0.01);
        assert!((content.height() - 405.0).abs() < 0.01);
        assert!((content.center() - container.center()).length() < 0.01);
    }

    #[test]
    fn focal_point_stays_under_the_pointer() {
        let video = video();
        let content = video;
        let focal = Pos2::new(900.0, 220.0);
        let mut view = ZoomView::default();
        let before = view.uv_at_screen(video, content, focal);
        view.zoom_at(video, content, focal, 2.5);
        let after = view.uv_at_screen(video, content, focal);
        assert!((before - after).length() < 0.001);
    }

    #[test]
    fn pan_is_clamped_to_the_video() {
        let video = video();
        let content = video;
        let mut view = ZoomView::default();
        view.zoom_at(video, content, video.center(), 2.0);
        view.pan_screen(content, Vec2::new(10_000.0, -10_000.0));
        let (offset, scale) = view.uv_transform();
        assert!((offset.x - 0.0).abs() < 0.001);
        assert!((offset.y - 0.0).abs() < 0.001);
        assert!((scale.x - 0.5).abs() < 0.001);
        assert!((scale.y - 0.5).abs() < 0.001);
    }

    #[test]
    fn zooming_out_resets_the_view() {
        let video = video();
        let content = video;
        let mut view = ZoomView::default();
        view.zoom_at(video, content, Pos2::new(100.0, 100.0), 4.0);
        view.zoom_at(video, content, Pos2::new(100.0, 100.0), 0.01);
        assert_eq!(view.zoom, MIN_ZOOM);
        assert_eq!(view.uv_transform().0, Vec2::ZERO);
        assert_eq!(view.uv_transform().1, Vec2::splat(1.0));
    }
}
