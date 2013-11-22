// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, Kang Seonghoon.
// See README.md for details.
//
// Licensed under the Apache License, Version 2.0 <http://www.apache.org/licenses/LICENSE-2.0> or
// the MIT license <http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Abstracted graphical screen.

use sdl::video;
use gfx::gl::{VertexBuffer, Texture2D, FrameBuffer};
use gfx::draw::{ProgramForShades, ProgramForTextures, ShadedDrawing, TexturedDrawing};
use gfx::bmfont::{Font, FontDrawingUtils, ShadedFontDrawing};
use gl = opengles::gl2;
use opengles::gl2::{GLint, GLfloat, GLsizei};

#[cfg(target_os="win32")] use opengles::egl;
#[cfg(target_os="win32")] use ext::sdl::syswm;

/// OpenGL state. This corresponds to EGL context in Windows; in the other platforms the global SDL
/// context handles this so there is no additional state.
#[cfg(target_os="win32")]
pub struct GLState {
    egl_display: egl::Display,
    egl_surface: egl::Surface,
    egl_context: egl::Context,
}

#[cfg(target_os="win32")]
impl GLState {
    /// Creates a new OpenGL state from the current SDL window.
    pub fn new() -> Result<GLState,~str> {
        #[fixed_stack_segment]; #[inline(never)];
        use ext::win32::ll::*;

        macro_rules! return_on_err(
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(err) => { return Err(format!("EGL error 0x{:x}", err)); }
                }
            }
        )

        // we need to preload this before initializing EGL
        do "d3dcompiler_43.dll".to_c_str().with_ref |dllname| {
            unsafe { LoadLibraryA(dllname); }
        }

        let hwnd = match syswm::get_wm_info() {
            Some(wminfo) => wminfo.window,
            None => { return Err(~"SDL_GetWMInfo failed"); }
        };
        let hdc = unsafe { GetDC(hwnd) };
        let display = return_on_err!(egl::get_display(hdc));
        return_on_err!(egl::initialize(display));

        let configattrs = [
            (egl::RED_SIZE, 8),
            (egl::GREEN_SIZE, 8),
            (egl::BLUE_SIZE, 8),
            (egl::ALPHA_SIZE, 8),
            (egl::DEPTH_SIZE, 16),
            (egl::STENCIL_SIZE, egl::DONT_CARE),
            (egl::SAMPLE_BUFFERS, 0),
        ];
        let configs = return_on_err!(egl::get_configs(display, configattrs, Some(1)));
        if configs.is_empty() {
            return Err(~"no suitable EGL configs available");
        }
        let config = configs[0];

        let surfaceattrs = [
            // none
        ];
        let surface = return_on_err!(egl::create_window_surface(display, config, hwnd,
                                                                surfaceattrs));

        let contextattrs = [
            (egl::CONTEXT_CLIENT_VERSION, 2),
        ];
        let context = return_on_err!(egl::create_context(display, config, None, contextattrs));

        return_on_err!(egl::make_current(display, surface, surface, context));
        Ok(GLState { egl_display: display, egl_surface: surface, egl_context: context })
    }

    /// Returns true if SDL's OpenGL support is in use.
    pub fn uses_sdl_ogl_support() -> bool { false }

    /// Swap the buffers if the double buffering is enabled. Do nothing otherwise.
    pub fn swap_buffers(&self) {
        egl::swap_buffers(self.egl_display, self.egl_surface);
    }
}

#[cfg(target_os="win32")]
impl Drop for GLState {
    fn drop(&mut self) {
        match egl::terminate(self.egl_display) {
            Ok(()) => {}
            Err(err) => fail!(format!("EGL error 0x{:x}", err))
        }
    }
}

/// OpenGL state. This corresponds to EGL context in Windows; in the other platforms the global SDL
/// context handles this so there is no additional state.
#[cfg(not(target_os="win32"))]
pub struct GLState;

#[cfg(not(target_os="win32"))]
impl GLState {
    /// Creates a new OpenGL state from the current SDL window.
    pub fn new() -> Result<GLState,~str> { Ok(GLState) }
    /// Returns true if SDL's OpenGL support is in use.
    pub fn uses_sdl_ogl_support() -> bool { true }
    /// Swap the buffers if the double buffering is enabled. Do nothing otherwise.
    pub fn swap_buffers(&self) { video::swap_buffers(); }
}

/// Abstracted graphical screen.
pub struct Screen {
    /// Screen width.
    width: uint,
    /// Screen height.
    height: uint,
    /// SDL surface returned by `sdl::video::set_video_mode`.
    sdl_surface: ~video::Surface,
    /// OpenGL state if required.
    glstate: GLState,
    /// Shared vertex buffer object for drawing.
    vertexbuf: VertexBuffer,
    /// OpenGL program for non-textured triangles.
    program_for_shades: ProgramForShades,
    /// OpenGL program for textured triangles.
    program_for_textures: ProgramForTextures,
    /// Shared bitmap font.
    font: Font,
}

impl Screen {
    /// Creates a new screen with given parameters.
    pub fn new(width: uint, height: uint, fullscreen: bool) -> Result<Screen,~str> {
        let mut surfaceflags;
        let mut videoflags;
        if fullscreen {
            surfaceflags = ~[];
            videoflags = ~[video::Fullscreen];
        } else {
            surfaceflags = ~[video::SWSurface];
            videoflags = ~[video::DoubleBuf];
        }
        if GLState::uses_sdl_ogl_support() {
            // SDL_OPENGL flag cannot be used in Windows as ANGLE should own the screen context.
            videoflags.push(video::OpenGL);
        }

        let screen = earlyexit!(video::set_video_mode(width as int, height as int, 32,
                                                      surfaceflags, videoflags));
        let glstate = earlyexit!(GLState::new());

        gl::enable(gl::BLEND);
        gl::blend_func(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
        gl::clear_color(0.0, 0.0, 0.0, 1.0);

        // initialize shaders
        let program_for_shades = earlyexit!(ProgramForShades::new());
        let program_for_textures = earlyexit!(ProgramForTextures::new());
        program_for_textures.bind();
        program_for_textures.sampler.set_1i(0);

        let screen = Screen { width: width, height: height, sdl_surface: screen,
                              glstate: glstate, vertexbuf: VertexBuffer::new(),
                              program_for_shades: program_for_shades,
                              program_for_textures: program_for_textures,
                              font: Font::new() };
        screen.set_local_transform([1.0, 0.0, 0.0,
                                    0.0, 1.0, 0.0,
                                    0.0, 0.0, 1.0]);
        screen.set_viewport(0, 0, width as GLsizei, height as GLsizei);
        Ok(screen)
    }

    /// Sets the local transform matrix. `matrix` should be a 3x3 row-major matrix.
    pub fn set_local_transform(&self, matrix: &[GLfloat]) {
        self.program_for_shades.bind();
        self.program_for_shades.local_transform.set_matrix_3f(false, matrix);
        self.program_for_textures.bind();
        self.program_for_textures.local_transform.set_matrix_3f(false, matrix);
    }

    /// Sets the viewport and corresponding projection matrix.
    pub fn set_viewport(&self, left: GLint, top: GLint, width: GLsizei, height: GLsizei) {
        gl::viewport(left, top, width, height);

        let (l, r) = (left as f32, left as f32 + width as f32);
        let (t, b) = (top as f32, top as f32 + height as f32);
        let (f, n) = (1.0, -1.0);
        let projection = [
            2.0/(r-l),    0.0,          0.0,          0.0,
            0.0,          2.0/(t-b),    0.0,          0.0,
            0.0,          0.0,          -2.0/(f-n),   0.0,
            -(r+l)/(r-l), -(t+b)/(t-b), -(f+n)/(f-n), 1.0,
        ];

        self.program_for_shades.bind();
        self.program_for_shades.projection.set_matrix_4f(false, projection);
        self.program_for_textures.bind();
        self.program_for_textures.projection.set_matrix_4f(false, projection);
    }

    /// Swap the buffers if the double buffering is enabled. Do nothing otherwise.
    pub fn swap_buffers(&self) { self.glstate.swap_buffers(); }

    /// Clears the whole screen.
    pub fn clear(&self) {
        gl::clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
    }

    /// Sets the scissor box within given block.
    /// Any draw operations inside the block will be clipped according to given scissor box.
    pub fn scissor(&self, x: int, y: int, w: uint, h: uint, f: &fn()) {
        assert!(!gl::is_enabled(gl::SCISSOR_TEST));
        gl::enable(gl::SCISSOR_TEST);
        gl::scissor(x as GLint, y as GLint, w as GLsizei, h as GLsizei);
        f();
        gl::disable(gl::SCISSOR_TEST);
    }

    /// Draws shaded primitives to the screen. The block receives a mutable reference to
    /// `util::gl::ShadedDrawing`, to which it should add points.
    pub fn draw_shaded_prim(&self, prim: gl::GLenum, f: &fn(&mut ShadedDrawing)) {
        let mut drawing = ShadedDrawing::new(prim);
        f(&mut drawing);
        drawing.draw_prim(&self.program_for_shades, &self.vertexbuf);
    }

    /// Same as `draw_shaded_prim` but with a default font.
    pub fn draw_shaded_prim_with_font(&self, prim: gl::GLenum, f: &fn(&mut ShadedFontDrawing)) {
        let mut drawing = ShadedDrawing::new(prim);
        drawing.with_font(&self.font, f);
        drawing.draw_prim(&self.program_for_shades, &self.vertexbuf);
    }

    /// Draws shaded triangles to the screen. The block receives a mutable reference to
    /// `util::gl::ShadedDrawing`, to which it should add points.
    pub fn draw_shaded(&self, f: &fn(&mut ShadedDrawing)) {
        self.draw_shaded_prim(gl::TRIANGLES, f)
    }

    /// Same as `draw_shaded` but with a default font.
    pub fn draw_shaded_with_font(&self, f: &fn(&mut ShadedFontDrawing)) {
        self.draw_shaded_prim_with_font(gl::TRIANGLES, f)
    }

    /// Draws textured primitives to the screen. The block receives a mutable reference to
    /// `util::gl::TexturedDrawing`, to which it should add points.
    pub fn draw_textured_prim(&self, prim: gl::GLenum, texture: &Texture2D,
                              f: &fn(&mut TexturedDrawing)) {
        let mut drawing = TexturedDrawing::new(prim, texture);
        f(&mut drawing);
        drawing.draw_prim(&self.program_for_textures, &self.vertexbuf, texture);
    }

    /// Draws textured triangles to the screen. The block receives a mutable reference to
    /// `util::gl::TexturedDrawing`, to which it should add points.
    pub fn draw_textured(&self, texture: &Texture2D, f: &fn(&mut TexturedDrawing)) {
        self.draw_textured_prim(gl::TRIANGLES, texture, f)
    }

    /// Renders to given frame buffer. The frame buffer should be complete.
    /// The viewport is temporarily reset to dimensions of the frame buffer in given block.
    pub fn render_to_framebuffer(&self, framebuf: &FrameBuffer, f: &fn(&Screen)) {
        assert!(framebuf.complete());
        let mut xywh: [GLint, ..4] = [0, 0, 0, 0];
        gl::get_integer_v(gl::VIEWPORT, xywh.mut_slice(0, 4));
        framebuf.bind();
        self.set_viewport(0, framebuf.height as GLint,
                          framebuf.width as GLsizei, -(framebuf.height as GLsizei));
        f(self);
        FrameBuffer::unbind();
        self.set_viewport(xywh[0], xywh[1], xywh[2] as GLsizei, xywh[3] as GLsizei);
    }
}
