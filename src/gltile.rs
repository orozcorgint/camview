use eframe::glow;
use glow::HasContext;

use crate::player::Player;

const VS: &str = r#"#version 330 core
out vec2 v_uv;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_uv = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const FS: &str = r#"#version 330 core
in vec2 v_uv;
out vec4 color;
uniform sampler2D tex;
void main() {
    color = texture(tex, v_uv);
}
"#;

/// FBO + shader that draws an mpv video frame inside a tile rectangle.
/// All methods are called with the current OpenGL context (paint callback).
pub struct TileGl {
    fbo: Option<glow::Framebuffer>,
    texture: Option<glow::Texture>,
    program: Option<glow::Program>,
    vao: Option<glow::VertexArray>,
    size: (i32, i32),
    failed: bool,
}

unsafe impl Send for TileGl {}

impl Default for TileGl {
    fn default() -> Self {
        Self {
            fbo: None,
            texture: None,
            program: None,
            vao: None,
            size: (0, 0),
            failed: false,
        }
    }
}

impl TileGl {
    fn compile_program(gl: &glow::Context) -> Option<(glow::Program, glow::VertexArray)> {
        unsafe {
            let vs = gl.create_shader(glow::VERTEX_SHADER).ok()?;
            gl.shader_source(vs, VS);
            gl.compile_shader(vs);
            if !gl.get_shader_compile_status(vs) {
                let log = gl.get_shader_info_log(vs);
                crate::platform::log_line(&format!("Shader vertex: {log}"));
                gl.delete_shader(vs);
                return None;
            }
            let fs = gl.create_shader(glow::FRAGMENT_SHADER).ok()?;
            gl.shader_source(fs, FS);
            gl.compile_shader(fs);
            if !gl.get_shader_compile_status(fs) {
                let log = gl.get_shader_info_log(fs);
                crate::platform::log_line(&format!("Shader fragment: {log}"));
                gl.delete_shader(vs);
                gl.delete_shader(fs);
                return None;
            }
            let program = gl.create_program().ok()?;
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            gl.detach_shader(program, vs);
            gl.detach_shader(program, fs);
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                crate::platform::log_line(&format!("Shader link: {log}"));
                gl.delete_program(program);
                return None;
            }
            let vao = gl.create_vertex_array().ok()?;
            Some((program, vao))
        }
    }

    fn ensure(&mut self, gl: &glow::Context, w: i32, h: i32) -> bool {
        if self.failed {
            return false;
        }
        unsafe {
            if self.program.is_none() {
                match Self::compile_program(gl) {
                    Some((p, v)) => {
                        self.program = Some(p);
                        self.vao = Some(v);
                    }
                    None => {
                        self.failed = true;
                        return false;
                    }
                }
            }
            if self.fbo.is_none() || self.size != (w, h) {
                if let Some(fbo) = self.fbo.take() {
                    gl.delete_framebuffer(fbo);
                }
                if let Some(tex) = self.texture.take() {
                    gl.delete_texture(tex);
                }
                let tex = match gl.create_texture() {
                    Ok(t) => t,
                    Err(e) => {
                        crate::platform::log_line(&format!("Textura video: {e}"));
                        self.failed = true;
                        return false;
                    }
                };
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    w,
                    h,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MIN_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_MAG_FILTER,
                    glow::LINEAR as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
                gl.bind_texture(glow::TEXTURE_2D, None);

                let fbo = match gl.create_framebuffer() {
                    Ok(f) => f,
                    Err(e) => {
                        crate::platform::log_line(&format!("FBO video: {e}"));
                        gl.delete_texture(tex);
                        self.failed = true;
                        return false;
                    }
                };
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                gl.framebuffer_texture_2d(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(tex),
                    0,
                );
                let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
                gl.bind_framebuffer(glow::FRAMEBUFFER, None);
                if status != glow::FRAMEBUFFER_COMPLETE {
                    crate::platform::log_line(&format!("FBO incompleto: 0x{status:x}"));
                    gl.delete_framebuffer(fbo);
                    gl.delete_texture(tex);
                    self.failed = true;
                    return false;
                }
                self.fbo = Some(fbo);
                self.texture = Some(tex);
                self.size = (w, h);
            }
        }
        true
    }

    pub fn draw(
        &mut self,
        gl: &glow::Context,
        player: &Player,
        vp_left: i32,
        vp_bottom: i32,
        vp_w: i32,
        vp_h: i32,
    ) {
        let w = vp_w.max(1);
        let h = vp_h.max(1);
        if !self.ensure(gl, w, h) {
            return;
        }
        let (Some(fbo), Some(tex), Some(program), Some(vao)) =
            (self.fbo, self.texture, self.program, self.vao)
        else {
            return;
        };

        unsafe {
            // Render mpv into the FBO. No scissor: coordinates belong to the FBO, not the screen.
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.viewport(0, 0, w, h);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
        }
        player.render_to_fbo(fbo.0.get() as i32, w, h);

        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            // Restore the viewport to the tile rectangle (egui set it before the
            // callback and mpv changed it to the FBO viewport).
            gl.viewport(vp_left, vp_bottom, vp_w, vp_h);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.use_program(Some(program));
            gl.bind_vertex_array(Some(vao));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            if let Some(loc) = gl.get_uniform_location(program, "tex") {
                gl.uniform_1_i32(Some(&loc), 0);
            }
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
    }
}
