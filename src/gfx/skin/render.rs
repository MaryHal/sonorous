// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, 2014, Kang Seonghoon.
// See README.md for details.
//
// Licensed under the Apache License, Version 2.0 <http://www.apache.org/licenses/LICENSE-2.0> or
// the MIT license <http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Skin renderer.

use std::{num, str, mem};
use std::rc::Rc;
use std::io::{IoResult, MemWriter};
use collections::HashMap;

use gl = opengles::gl2;
use sdl_image;

use gfx::gl::Texture2D;
use gfx::draw::{ShadedDrawing, ShadedDrawingTraits, TexturedDrawing, TexturedDrawingTraits};
use gfx::bmfont::{NCOLUMNS, NROWS, FontDrawingUtils, LeftAligned};
use gfx::screen::{Screen, ShadedFontDrawing, ScreenDraw, ScreenTexturedDraw};
use gfx::skin::scalar::{Scalar, TextureScalar, ImageScalar};
use gfx::skin::ast::{Expr, ENum, ERatioNum, Pos, Rect};
use gfx::skin::ast::{Gen, HookGen, TextGen, TextLenGen};
use gfx::skin::ast::{Block, CondBlock, MultiBlock};
use gfx::skin::ast::{ScalarFormat, NoFormat, NumFormat, MsFormat, HmsFormat};
use gfx::skin::ast::{TextSource, ScalarText, StaticText, TextBlock, TextConcat};
use gfx::skin::ast::{Node, Nothing, Debug, ColoredLine, ColoredRect, TexturedRect,
                     Text, Group, Clip};
use gfx::skin::ast::{Skin};
use gfx::skin::hook::Hook;

/// The currently active draw call.
enum ActiveDraw<'a> {
    /// No draw call active.
    ToBeDrawn,
    /// Shaded drawing with `GL_LINES` primitive.
    ShadedLines(ShadedDrawing),
    /// Shaded drawing with `GL_TRIANGLES` primitive.
    Shaded(ShadedFontDrawing), // TODO it will eventually need font refs
    /// Textured drawing with `GL_TRIANGLES` primitive.
    /// It also stores the reference to the texture for commit.
    Textured(TexturedDrawing, Rc<Texture2D>),
}

/// The skin renderer.
pub struct Renderer {
    /// The current skin data.
    skin: Rc<Skin>,
    /// The cached textures for image scalars. `None` indicates the error without further retries.
    imagecache: HashMap<Path,Option<Rc<Texture2D>>>,
}

impl Renderer {
    /// Creates a new renderer out of the skin data.
    /// The renderer maintains the global cache, thus should be kept as long as possible.
    pub fn new(skin: Skin) -> Renderer {
        Renderer { skin: Rc::new(skin), imagecache: HashMap::new() }
    }

    /// Renders the skin with supplied hook to given screen.
    /// This overwrites to the current screen, no `clear` is called.
    pub fn render(&mut self, screen: &mut Screen, hook: &Hook) {
        let skin = self.skin.clone();
        let mut state = State::new(self, screen);
        state.nodes(hook, skin.nodes.as_slice());
        state.finish();
    }
}

/// The internal state for single render.
struct State<'a> {
    /// Parent renderer.
    renderer: &'a mut Renderer,
    /// Screen reference.
    screen: &'a mut Screen,
    /// The active draw call.
    draw: ActiveDraw<'a>,
    /// Left coordinate of the clipping region.
    dx: f32,
    /// Top coordinate of the clipping region.
    dy: f32,
    /// The width of the clipping region.
    w: f32,
    /// The height of the clipping region.
    h: f32,
}

impl<'a> State<'a> {
    fn new(renderer: &'a mut Renderer, screen: &'a mut Screen) -> State<'a> {
        State { renderer: renderer, screen: screen, draw: ToBeDrawn,
                dx: 0.0, dy: 0.0, w: screen.width as f32, h: screen.height as f32 }
    }

    fn expr(_hook: &Hook, pos: &Expr, reference: f32) -> f32 {
        match *pos {
            ENum(v) => v,
            ERatioNum(r, v) => r * reference + v,
        }
    }

    fn pos(&self, hook: &Hook, pos: &Pos) -> (f32, f32) {
        (self.dx + State::expr(hook, &pos.x, self.w),
         self.dy + State::expr(hook, &pos.y, self.h))
    }

    fn rect(&self, hook: &Hook, rect: &Rect) -> ((f32, f32), (f32, f32)) {
        (self.pos(hook, &rect.p), self.pos(hook, &rect.q))
    }

    fn gen(hook: &Hook, gen: &Gen, body: |&Hook, &str| -> bool) -> bool {
        match *gen {
            HookGen(ref id) => hook.block_hook(id.as_slice(), hook, body),
            TextGen(ref id) => match hook.scalar_hook(id.as_slice()) {
                Some(scalar) => {
                    let text = scalar.into_maybe_owned();
                    body(hook, text.as_slice());
                    true
                },
                None => false
            },
            TextLenGen(ref id) => match hook.scalar_hook(id.as_slice()) {
                Some(scalar) => {
                    let text = scalar.into_maybe_owned();
                    if !text.is_empty() {
                        body(hook, text.as_slice().char_len().to_str());
                    }
                    true
                },
                None => false
            },
        }
    }

    fn block<T>(hook: &Hook, block: &Block<T>, body: |&Hook, &T| -> bool) {
        match *block {
            CondBlock { ref gen, ref then, ref else_ } => {
                let mut called = false;
                State::gen(hook, gen, |hook_, _alt| {
                    called = true;
                    match *then {
                        Some(ref then) => body(hook_, then),
                        None => true,
                    }
                });
                if !called {
                    match *else_ {
                        Some(ref else_) => { body(hook, else_); }
                        None => {}
                    }
                }
            }
            MultiBlock { ref gen, ref map, ref default, ref else_ } => {
                let mut called = false;
                State::gen(hook, gen, |hook_, alt| {
                    called = true;
                    match map.find_equiv(&alt) {
                        Some(then) => body(hook_, then),
                        None => match *default {
                            Some(ref default) => body(hook_, default),
                            None => {
                                warn!("skin: `{id}` gave an unknown alternative `{alt}`, \
                                       will ignore", id = gen.id(), alt = alt);
                                true
                            }
                        }
                    }
                });
                if !called {
                    match *else_ {
                        Some(ref else_) => { body(hook, else_); }
                        None => {}
                    }
                }
            }
        }
    }

    fn scalar_format<'a>(scalar: Scalar<'a>, fmt: &ScalarFormat,
                         out: &mut Writer) -> IoResult<()> {
        fn to_f64<'a>(scalar: Scalar<'a>) -> Option<f64> {
            let v = scalar.to_f64();
            if v.is_none() {
                warn!("skin: scalar_format received a non-number `{}`, will ignore", scalar);
            }
            v
        }

        fn fill_and_clip(out: &mut Writer, sign: bool, minwidth: u8, maxwidth: u8,
                         precision: u8, v: f64) -> IoResult<()> {
            let precision = precision as uint;
            if maxwidth == 255 && minwidth == 0 {
                // no need to construct a temporary buffer
                if sign {
                    write!(out, "{:+.*}", precision, v)
                } else {
                    write!(out, "{:.*}", precision, v)
                }
            } else {
                let maxwidth = maxwidth as uint;
                let minwidth = minwidth as uint;
                let s = if sign {
                    format!("{:+.*}", precision, v)
                } else {
                    format!("{:.*}", precision, v)
                };
                let ss = if s.len() > maxwidth {
                    s.slice_from(s.len() - maxwidth)
                } else {
                    s.as_slice()
                };
                if ss.len() < minwidth {
                    let signidx = if ss.starts_with("+") || ss.starts_with("-") {1} else {0};
                    if signidx > 0 {
                        try!(write!(out, "{}", ss.slice_to(signidx)));
                    }
                    for _ in range(0, minwidth - ss.len()) {
                        try!(write!(out, "0"));
                    }
                    write!(out, "{}", ss.slice_from(signidx))
                } else {
                    write!(out, "{}", ss)
                }
            }
        }

        match *fmt {
            NoFormat => write!(out, "{}", scalar),
            NumFormat { sign, minwidth, maxwidth, precision, multiplier } => {
                let v = match to_f64(scalar) { Some(v) => v, None => return Ok(()) };
                let v = v * multiplier as f64;
                fill_and_clip(out, sign, minwidth, maxwidth, precision, v)
            },
            MsFormat { sign, minwidth, maxwidth, precision, multiplier } => {
                let v = match to_f64(scalar) { Some(v) => v, None => return Ok(()) };
                let v = v * multiplier as f64;
                let (min, sec) = num::div_rem(v, 60.0);
                try!(fill_and_clip(out, sign, minwidth, maxwidth, 0, min));
                write!(out, ":{:02.*}", precision as uint, sec.abs())
            },
            HmsFormat { sign, minwidth, maxwidth, precision, multiplier } => {
                let v = match to_f64(scalar) { Some(v) => v, None => return Ok(()) };
                let v = v * multiplier as f64;
                let (min, sec) = num::div_rem(v, 60.0);
                let (hour, min) = num::div_rem(min, 60.0);
                try!(fill_and_clip(out, sign, minwidth, maxwidth, 0, hour));
                write!(out, ":{:02}:{:02.*}", min.abs(), precision as uint, sec.abs())
            },
        }
    }

    fn text_source<'a>(hook: &'a Hook, text: &'a TextSource, out: &mut Writer) -> IoResult<()> {
        match *text {
            ScalarText(ref id, ref format) => match hook.scalar_hook(id.as_slice()) {
                Some(scalar) => State::scalar_format(scalar, format, out),
                _ => {
                    warn!("skin: `{id}` is not a scalar hook, will use an empty string",
                          id = id.as_slice());
                    Ok(())
                },
            },
            StaticText(ref text) => write!(out, "{}", *text),
            TextBlock(ref block) => {
                let mut ret = Ok(());
                State::block(hook, block, |hook_, text_| {
                    match State::text_source(hook_, *text_, out) {
                        Ok(()) => true,
                        Err(err) => { ret = Err(err); false }
                    }
                });
                ret
            },
            TextConcat(ref nodes) => {
                for node in nodes.iter() {
                    try!(State::text_source(hook, node, out));
                }
                Ok(())
            },
        }
    }

    fn text<'a>(hook: &'a Hook, text: &'a TextSource) -> str::MaybeOwned<'a> {
        let mut out = MemWriter::new();
        match State::text_source(hook, text, &mut out) {
            Ok(()) => {
                str::from_utf8(out.unwrap().as_slice()).unwrap().to_owned().into_maybe_owned()
            },
            Err(err) => {
                warn!("skin: I/O error on text_source({}), will ignore", err);
                "".into_maybe_owned()
            }
        }
    }

    // unlike others, this has to be a non-static method as we need the image cache
    fn texture<'a>(&'a mut self, hook: &'a Hook, id: &str) -> Option<&'a Rc<Texture2D>> {
        let mut scalar = hook.scalar_hook(id);
        if scalar.is_none() {
            scalar = self.renderer.skin.scalars.find_equiv(&id).map(|v| v.clone());
        }
        match scalar {
            Some(TextureScalar(tex)) => Some(tex),
            Some(ImageScalar(path)) => {
                let ret = self.renderer.imagecache.find_or_insert_with(path, |path| {
                    match sdl_image::load(path) {
                        Ok(surface) => match Texture2D::from_owned_surface(surface, false, false) {
                            Ok(tex) => Some(Rc::new(tex)),
                            Err(..) => None,
                        },
                        Err(..) => None,
                    }
                });
                match *ret {
                    Some(ref tex) => Some(tex),
                    None => None
                }
            },
            _ => None,
        }
    }

    fn commit(&mut self, draw: ActiveDraw) {
        match draw {
            ToBeDrawn => {}
            ShadedLines(d) => { d.draw_to(self.screen); }
            Shaded(d) => { d.draw_to(self.screen); }
            Textured(d, tex) => { d.draw_texture_to(self.screen, tex.deref()); }
        }
    }

    fn finish(&mut self) {
        let draw = mem::replace(&mut self.draw, ToBeDrawn);
        self.commit(draw);
    }

    fn shaded_lines<'a>(&'a mut self) -> &'a mut ShadedDrawing {
        match self.draw {
            ShadedLines(ref mut d) => { return d; }
            _ => {
                let newd = ShadedDrawing::new(gl::LINES);
                let draw = mem::replace(&mut self.draw, ShadedLines(newd));
                self.commit(draw);
                match self.draw {
                    ShadedLines(ref mut d) => d,
                    _ => unreachable!()
                }
            }
        }
    }

    fn shaded<'a>(&'a mut self) -> &'a mut ShadedFontDrawing {
        match self.draw {
            Shaded(ref mut d) => { return d; }
            _ => {
                let newd = ShadedFontDrawing::new(gl::TRIANGLES, self.screen.font.clone());
                let draw = mem::replace(&mut self.draw, Shaded(newd));
                self.commit(draw);
                match self.draw {
                    Shaded(ref mut d) => d,
                    _ => unreachable!()
                }
            }
        }
    }

    fn textured<'a>(&'a mut self, tex: &Rc<Texture2D>) -> &'a mut TexturedDrawing {
        match self.draw {
            // keep the current drawing only when the texture is exactly identical.
            Textured(ref mut d, ref tex_)
                    if tex.deref() as *Texture2D == tex_.deref() as *Texture2D => {
                return d;
            }
            _ => {
                let tex = tex.clone();
                let newd = TexturedDrawing::new(gl::TRIANGLES, tex.deref());
                let draw = mem::replace(&mut self.draw, Textured(newd, tex));
                self.commit(draw);
                match self.draw {
                    Textured(ref mut d, _) => d,
                    _ => unreachable!()
                }
            }
        }
    }

    fn nodes(&mut self, hook: &Hook, nodes: &[Node]) -> bool {
        for node in nodes.iter() {
            match *node {
                Nothing => {}
                Debug(ref msg) => {
                    debug!("skin debug: dx={} dy={} w={} y={} msg={}",
                           self.dx, self.dy, self.w, self.h, *msg);
                }
                ColoredLine { ref from, ref to, ref color } => {
                    let (x1, y1) = self.pos(hook, from);
                    let (x2, y2) = self.pos(hook, to);
                    self.shaded_lines().line(x1, y1, x2, y2, *color);
                }
                ColoredRect { ref at, ref color } => {
                    let ((x1, y1), (x2, y2)) = self.rect(hook, at);
                    self.shaded().rect(x1, y1, x2, y2, *color);
                }
                TexturedRect { ref tex, ref at, ref rgba, ref clip } => {
                    let ((x1, y1), (x2, y2)) = self.rect(hook, at);
                    let texture = {
                        let tex = self.texture(hook, tex.as_slice());
                        tex.map(|tex| tex.clone()) // XXX should avoid the clone here
                    };
                    match (texture, *clip) {
                        (Some(ref tex), Some(ref clip)) => {
                            let (tw, th) = (tex.width as f32, tex.height as f32);
                            let tx1 = State::expr(hook, &clip.p.x, tw);
                            let ty1 = State::expr(hook, &clip.p.y, th);
                            let tx2 = State::expr(hook, &clip.q.x, tw);
                            let ty2 = State::expr(hook, &clip.q.y, th);
                            self.textured(tex).rect_area_rgba(x1, y1, x2, y2,
                                                              tx1, ty1, tx2, ty2, *rgba);
                        }
                        (Some(ref tex), None) => {
                            self.textured(tex).rect_rgba(x1, y1, x2, y2, *rgba);
                        }
                        (_, _) => {
                            warn!("skin: `{id}` is not a texture hook, will ignore",
                                  id = tex.as_slice());
                        }
                    }
                }
                Text { ref at, size, anchor: (ax,ay), ref color, ref text } => {
                    let (x, y) = self.pos(hook, at);
                    let text = State::text(hook, text);
                    let zoom = size / NROWS as f32;
                    let w = zoom * (text.as_slice().char_len() * NCOLUMNS) as f32;
                    let h = size as f32;
                    self.shaded().string(x - w * ax, y - h * ay, zoom,
                                         LeftAligned, text.as_slice(), *color);
                }
                Group(ref nodes) => {
                    let dx = self.dx;
                    let dy = self.dy;
                    let w = self.w;
                    let h = self.h;
                    self.nodes(hook, nodes.as_slice());
                    self.dx = dx;
                    self.dy = dy;
                    self.w = w;
                    self.h = h;
                }
                Clip { ref at } => {
                    let ((x1, y1), (x2, y2)) = self.rect(hook, at);
                    self.dx = x1;
                    self.dy = y1;
                    self.w = x2 - x1;
                    self.h = y2 - y1;
                    if self.w <= 0.0 || self.h <= 0.0 {
                        // no need to render past this node.
                        // XXX this should propagate to parent nodes up to the innermost group
                        return false;
                    }
                }
                Block(ref block) => {
                    State::block(hook, block, |hook_, nodes_| {
                        self.nodes(hook_, nodes_.as_slice())
                    });
                }
            }
        }
        true
    }
}
