// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, 2014, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! Game play screen. Renders the screen from `engine::player::Player` state.

use std::{cmp, iter};
use std::num::SignedInt;
use std::rc::Rc;
use std::cell::RefCell;

use format::obj::{Lane, Visible, LNStart, LNDone, Bomb, MeasureBar};
use format::obj::{BGALayer};
use format::obj::{ObjLoc, ObjAxis, ObjQueryOps};
use gfx::color::{Color, Gradient, RGB, RGBA, Blend};
use gfx::surface::{Surface, SurfaceAreaUtil, SurfacePixelsUtil};
use gfx::gl::{Texture2D, PreparedSurface};
use gfx::draw::{ShadedDrawing, ShadedDrawingTraits, TexturedDrawing, TexturedDrawingTraits};
use gfx::bmfont::{FontDrawingUtils, Alignment};
use gfx::screen::Screen;
use engine::keyspec::{KeyKind, KeySpec};
use engine::resource::{BGAW, BGAH, Imagelike};
use engine::player::{Grade, MAXGAUGE, Player};
use ui::init::{SCREENW, SCREENH};
use ui::scene::{Scene, SceneOptions, SceneCommand};
use ui::viewing::BGACanvas;
use ui::playresult::PlayResultScene;

/// An appearance for each lane.
pub struct LaneStyle {
    /// The left position of the lane in the final screen.
    pub left: uint,
    /// The left position of the lane in the object sprite.
    pub spriteleft: uint,
    /// The left position of the lane in the bomb sprite.
    pub spritebombleft: uint,
    /// The width of lane.
    pub width: uint,
    /// The base color of object. The actual `Gradient` for drawing is derived from this color.
    pub basecolor: Color
}

impl LaneStyle {
    /// Constructs a new `LaneStyle` object from given key kind and the left or right position.
    pub fn from_kind(kind: KeyKind, pos: uint, right: bool) -> LaneStyle {
        let (spriteleft, spritebombleft, width, color) = match kind {
            KeyKind::WhiteKey    => ( 25,   0, 25, RGB(0x80,0x80,0x80)),
            KeyKind::WhiteKeyAlt => ( 50,   0, 25, RGB(0xf0,0xe0,0x80)),
            KeyKind::BlackKey    => ( 75,   0, 25, RGB(0x80,0x80,0xff)),
            KeyKind::Button1     => (130, 100, 30, RGB(0xe0,0xe0,0xe0)),
            KeyKind::Button2     => (160, 100, 30, RGB(0xff,0xff,0x40)),
            KeyKind::Button3     => (190, 100, 30, RGB(0x80,0xff,0x80)),
            KeyKind::Button4     => (220, 100, 30, RGB(0x80,0x80,0xff)),
            KeyKind::Button5     => (250, 100, 30, RGB(0xff,0x40,0x40)),
            KeyKind::Scratch     => (320, 280, 40, RGB(0xff,0x80,0x80)),
            KeyKind::FootPedal   => (360, 280, 40, RGB(0x80,0xff,0x80)),
        };
        let left = if right {pos - width} else {pos};
        LaneStyle { left: left, spriteleft: spriteleft, spritebombleft: spritebombleft,
                    width: width, basecolor: color }
    }

    /// Renders required object and bomb images to the sprite.
    pub fn render_to_sprite(&self, sprite: &Surface) {
        let left = self.spriteleft;
        let noteleft = self.spriteleft + SCREENW;
        let bombleft = self.spritebombleft + SCREENW;
        assert!(sprite.get_width() as uint >= cmp::max(noteleft, bombleft) + self.width);

        // render a background sprite (0 at top, <1 at bottom)
        let backcolor = Gradient { zero: RGB(0,0,0), one: self.basecolor };
        for i in range(140u, SCREENH - 80) {
            sprite.fill_area((left, i), (self.width, 1u), backcolor.blend(i as int - 140, 1000));
        }

        // render note and bomb sprites (1/2 at middle, 1 at border)
        let denom = self.width as int;
        let notecolor = Gradient { zero: RGB(0xff,0xff,0xff), one: self.basecolor };
        let bombcolor = Gradient { zero: RGB(0,0,0),          one: RGB(0xc0,0,0) };
        for i in range(0, self.width / 2) {
            let num = (self.width - i) as int;
            sprite.fill_area((noteleft+i, 0u), (self.width-i*2, SCREENH),
                             notecolor.blend(num, denom));
            sprite.fill_area((bombleft+i, 0u), (self.width-i*2, SCREENH),
                             bombcolor.blend(num, denom));
        }
    }

    /// Clears the lane background.
    pub fn clear_back(&self, d: &mut ShadedDrawing) {
        d.rect(self.left as f32, 30.0,
               (self.left + self.width) as f32, SCREENH as f32 - 80.0, RGB(0,0,0));
    }

    /// Renders the key-pressed lane background to the screen from the sprite.
    pub fn render_pressed_back(&self, d: &mut TexturedDrawing) {
        d.rect_area(self.left as f32, 140.0,
                    (self.left + self.width) as f32, SCREENH as f32 - 80.0,
                    self.spriteleft as f32, 140.0,
                    (self.spriteleft + self.width) as f32, SCREENH as f32 - 80.0);
    }

    /// Renders an object to the screen from the sprite.
    pub fn render_note(&self, d: &mut TexturedDrawing, top: f32, bottom: f32) {
        d.rect_area(self.left as f32, top,
                    (self.left + self.width) as f32, bottom,
                    (self.spriteleft + SCREENW) as f32, 0.0,
                    (self.spriteleft + self.width + SCREENW) as f32, bottom);
    }

    /// Renders an elongated object to the screen from the sprite.
    pub fn render_longnote(&self, d: &mut TexturedDrawing, top: f32, bottom: f32, alpha: u8) {
        d.rect_area_rgba(self.left as f32, top,
                         (self.left + self.width) as f32, bottom,
                         (self.spriteleft + SCREENW) as f32, 0.0,
                         (self.spriteleft + self.width + SCREENW) as f32, bottom,
                         (255,255,255,alpha));
    }

    /// Renders a bomb object to the screen from the sprite.
    pub fn render_bomb(&self, d: &mut TexturedDrawing, top: f32, bottom: f32) {
        d.rect_area(self.left as f32, top,
                    (self.left + self.width) as f32, bottom,
                    (self.spritebombleft + SCREENW) as f32, 0.0,
                    (self.spritebombleft + self.width + SCREENW) as f32, bottom);
    }
}

/// Builds a list of `LaneStyle`s from the key specification.
fn build_lane_styles(keyspec: &KeySpec) ->
                                Result<(uint, Option<uint>, Vec<(Lane,LaneStyle)>), String> {
    let mut leftmost = 0;
    let mut rightmost = SCREENW;
    let mut styles = Vec::new();
    for &lane in keyspec.left_lanes().iter() {
        let kind = keyspec.kinds[*lane];
        assert!(kind.is_some());
        let kind = kind.unwrap();
        let style = LaneStyle::from_kind(kind, leftmost, false);
        styles.push((lane, style));
        leftmost += style.width + 1;
        if leftmost > SCREENW - 20 {
            return Err(format!("The screen can't hold that many lanes"));
        }
    }
    for &lane in keyspec.right_lanes().iter().rev() {
        let kind = keyspec.kinds[*lane];
        assert!(kind.is_some());
        let kind = kind.unwrap();
        let style = LaneStyle::from_kind(kind, rightmost, true);
        styles.push((lane, style));
        if rightmost < leftmost + 40 {
            return Err(format!("The screen can't hold that many lanes"));
        }
        rightmost -= style.width + 1;
    }
    let mut rightmost = if rightmost == SCREENW {None} else {Some(rightmost)};

    // move lanes to the center if there are too small number of lanes
    let cutoff = 165;
    if leftmost < cutoff {
        for i in range(0, keyspec.split) {
            let (_lane, ref mut style) = styles[mut][i];
            style.left += (cutoff - leftmost) / 2;
        }
        leftmost = cutoff;
    }
    if rightmost.map_or(false, |x| x > SCREENW - cutoff) {
        for i in range(keyspec.split, styles.len()) {
            let (_lane, ref mut style) = styles[mut][i];
            style.left -= (rightmost.unwrap() - (SCREENW - cutoff)) / 2;
        }
        rightmost = Some(SCREENW - cutoff);
    }

    Ok((leftmost, rightmost, styles))
}

/// Creates a sprite.
fn create_sprite(leftmost: uint, rightmost: Option<uint>,
                 styles: &[(Lane,LaneStyle)]) -> Texture2D {
    let sprite = match PreparedSurface::new(SCREENW + 400, SCREENH, true) {
        Ok(PreparedSurface(surface)) => surface,
        Err(err) => die!("PreparedSurface::new failed: {}", err)
    };

    // render notes and lane backgrounds
    for &(_lane,style) in styles.iter() {
        style.render_to_sprite(&sprite);
    }

    // render panels
    sprite.with_pixels(|pixels| {
        let topgrad = Gradient { zero: RGB(0x60,0x60,0x60), one: RGB(0xc0,0xc0,0xc0) };
        let botgrad = Gradient { zero: RGB(0x40,0x40,0x40), one: RGB(0xc0,0xc0,0xc0) };
        for j in range(-244i, 556) {
            for i in range(-10i, 20) {
                let c = (i*2+j*3+750) % 2000;
                pixels.put_pixel((j+244) as uint, (i+10) as uint,
                                 topgrad.blend(850 - (c-1000).abs(), 700));
            }
            for i in range(-20i, 60) {
                let c = (i*3+j*2+750) % 2000;
                let bottom = (SCREENH - 60) as int;
                pixels.put_pixel((j+244) as uint, (i+bottom) as uint,
                                 botgrad.blend(850 - (c-1000).abs(), 700));
            }
        }
    });
    sprite.fill_area((10u, SCREENH-36), (leftmost, 1u), RGB(0x40,0x40,0x40));

    // erase portions of panels left unused
    let leftgap = leftmost + 20;
    let rightgap = rightmost.map_or(SCREENW, |x| x - 20);
    let gapwidth = rightgap - leftgap;
    let black = RGB(0,0,0);
    sprite.fill_area((leftgap, 0u), (gapwidth, 30u), black);
    sprite.fill_area((leftgap, SCREENH-80), (gapwidth, 80u), black);
    sprite.with_pixels(|pixels| {
        for i in range(0u, 20) {
            // Rust: this cannot be `uint` since `-1u` underflows!
            for j in iter::range_step(20i, 0, -1) {
                let j = j as uint;
                if i*i + j*j <= 400 { break; } // circled border
                pixels.put_pixel(leftmost + j, 10 + i, black);
                pixels.put_pixel(leftmost + j, (SCREENH-61) - i, black);
                for &right in rightmost.iter() {
                    pixels.put_pixel((right-j) - 1, 10 + i, black);
                    pixels.put_pixel((right-j) - 1, (SCREENH-61) - i, black);
                }
            }
        }
    });

    match Texture2D::from_owned_surface(sprite, false, false) {
        Ok(tex) => tex,
        Err(err) => die!("Texture2D::from_owned_surface failed: {}", err)
    }
}

/// Game play scene context. Used for the normal game play and automatic play mode.
pub struct PlayingScene {
    /// Game play state with various non-graphic resources.
    pub player: Player,
    /// Sprite texture generated by `create_sprite`.
    pub sprite: Texture2D,
    /// Display screen.
    pub screen: Rc<RefCell<Screen>>,
    /// Image resources.
    pub imgres: Vec<Imagelike>,

    /// The leftmost X coordinate of the area next to the lanes, that is, the total width of
    /// left-hand-side lanes.
    pub leftmost: uint,
    /// The rightmost X coordinate of the area next to the lanes, that is, the screen width
    /// minus the total width of right-hand-side lanes if any. `None` indicates the absence of
    /// right-hand-side lanes.
    pub rightmost: Option<uint>,
    /// The order and appearance of lanes.
    pub lanestyles: Vec<(Lane,LaneStyle)>,
    /// The left coordinate of the BGA.
    pub bgax: uint,
    /// The top coordinate of the BGA.
    pub bgay: uint,

    /// If not `None`, indicates that the POOR BGA should be displayed until this timestamp.
    pub poorlimit: Option<uint>,
    /// If not `None`, indicates that the grading information should be displayed until
    /// this timestamp.
    pub gradelimit: Option<uint>,
    /// BGA canvas.
    pub bgacanvas: BGACanvas,
}

impl PlayingScene {
    /// Creates a new game play scene from the player, pre-allocated (usually by `init_video`)
    /// screen and pre-loaded image resources. Other resources including pre-loaded sound resources
    /// are included in the `player`.
    pub fn new(player: Player, screen: Rc<RefCell<Screen>>,
               imgres: Vec<Imagelike>) -> Result<Box<PlayingScene>,String> {
        let (leftmost, rightmost, styles) = match build_lane_styles(&player.keyspec) {
            Ok(styles) => styles,
            Err(err) => { return Err(err); }
        };
        let centerwidth = rightmost.unwrap_or(SCREENW) - leftmost;
        let bgax = leftmost + (centerwidth - BGAW) / 2;
        let bgay = (SCREENH - BGAH) / 2;
        let sprite = create_sprite(leftmost, rightmost, styles[]);
        let bgacanvas = BGACanvas::new(imgres[]);

        Ok(box PlayingScene {
            player: player, sprite: sprite, screen: screen, imgres: imgres,
            leftmost: leftmost, rightmost: rightmost, lanestyles: styles, bgax: bgax, bgay: bgay,
            poorlimit: None, gradelimit: None, bgacanvas: bgacanvas,
        })
    }
}

/// The list of grade names and corresponding color scheme.
pub static GRADES: &'static [(&'static str,Gradient)] = &[
    ("MISS",  Gradient { zero: RGB(0xff,0xc0,0xc0), one: RGB(0xff,0x40,0x40) }),
    ("BAD",   Gradient { zero: RGB(0xff,0xc0,0xff), one: RGB(0xff,0x40,0xff) }),
    ("GOOD",  Gradient { zero: RGB(0xff,0xff,0xc0), one: RGB(0xff,0xff,0x40) }),
    ("GREAT", Gradient { zero: RGB(0xc0,0xff,0xc0), one: RGB(0x40,0xff,0x40) }),
    ("COOL",  Gradient { zero: RGB(0xc0,0xc0,0xff), one: RGB(0x40,0x40,0xff) }),
];

impl Scene for PlayingScene {
    fn activate(&mut self) -> SceneCommand { SceneCommand::Continue }

    fn scene_options(&self) -> SceneOptions { SceneOptions::new() }

    fn tick(&mut self) -> SceneCommand {
        // TODO `QuitEvent` should be handled by the scene and not the player!
        if self.player.tick() {
            // update display states
            for &(grade,when) in self.player.lastgrade.iter() {
                if grade == Grade::MISS {
                    // switches to the normal BGA after 600ms
                    let minlimit = when + 600;
                    self.poorlimit =
                        Some(self.poorlimit.map_or(minlimit, |t| cmp::max(t, minlimit)));
                }
                // grade disappears after 700ms
                let minlimit = when + 700;
                self.gradelimit =
                    Some(self.gradelimit.map_or(minlimit, |t| cmp::max(t, minlimit)));
            }
            if self.poorlimit < Some(self.player.now) { self.poorlimit = None; }
            if self.gradelimit < Some(self.player.now) { self.gradelimit = None; }
            self.bgacanvas.update(&self.player.bga, self.imgres[]);

            SceneCommand::Continue
        } else {
            if self.player.opts.is_autoplay() { return SceneCommand::Pop; }

            // check if the song reached the last gradable object (otherwise the game play was
            // terminated by the user)
            let nextgradable = self.player.cur.find_next_of_type(|obj| obj.is_gradable());
            if nextgradable.is_some() { return SceneCommand::Pop; }

            // otherwise move to the result screen
            SceneCommand::Replace
        }
    }

    fn render(&self) {
        let mut screen = self.screen.borrow_mut();

        const W: f32 = SCREENW as f32;
        const H: f32 = SCREENH as f32;

        let beat = self.player.cur.loc.vpos * 4.0 % 1.0;

        screen.clear();

        // render BGAs (should render before the lanes since lanes can overlap with BGAs)
        if self.player.opts.has_bga() {
            static POOR_LAYERS: [BGALayer, ..1] = [BGALayer::PoorBGA];
            static NORM_LAYERS: [BGALayer, ..3] = [BGALayer::Layer1, BGALayer::Layer2,
                                                   BGALayer::Layer3];
            let layers = if self.poorlimit.is_some() {POOR_LAYERS[]} else {NORM_LAYERS[]};
            self.bgacanvas.render_to_texture(screen.deref_mut(), layers);
            screen.draw_textured(self.bgacanvas.as_texture(), |d| {
                d.rect(self.bgax as f32, self.bgay as f32,
                       (self.bgax + BGAW) as f32, (self.bgay + BGAH) as f32);
            });
        }

        screen.draw_shaded(|d| {
            // fill the lanes to the border color
            d.rect(0.0, 30.0, self.leftmost as f32, H-80.0, RGB(0x40,0x40,0x40));
            for &rightmost in self.rightmost.iter() {
                d.rect(rightmost as f32, 30.0, W, 520.0, RGB(0x40,0x40,0x40));
            }

            // clear the lanes to the background color
            for &(_lane,style) in self.lanestyles.iter() {
                style.clear_back(d);
            }
        });

        // basically, we use a window of 1.25 measures in the actual position, but then we will
        // hide the topmost and bottommost 5 pixels behind the panels (for avoiding vanishing notes)
        // and move the grading line accordingly. this bias represents the amount of such moves.
        let bias = (6.25 / (H-100.0)) as f64; // H-100:1.25 = 5:bias
        let bottom = self.player.cur.find(ObjAxis::ActualPos, -bias / self.player.playspeed);
        let top = self.player.cur.find(ObjAxis::ActualPos, (1.25 - bias) / self.player.playspeed);

        let loc_to_y = |loc: &ObjLoc<f64>| {
            let offset = loc.pos - self.player.cur.loc.pos;
            (H-80.0) - ((H-100.0)/1.25 * self.player.playspeed as f32 * offset as f32)
        };

        screen.draw_textured(&self.sprite, |d| {
            // if we are in the reverse motion, do not draw objects before the motion start.
            let localbottom = match self.player.reverse {
                Some(ref reverse) => reverse.clone(),
                None => bottom.clone(),
            };

            // render objects
            for &(lane,style) in self.lanestyles.iter() {
                if self.player.key_pressed(lane) { style.render_pressed_back(d); }

                let front = localbottom.find_next_of_type(|obj| {
                    obj.object_lane() == Some(lane) && obj.is_renderable()
                });
                if front.is_none() { continue; }
                let front = front.unwrap();

                // LN starting before the bottom and ending after the top
                let lnalpha = (150.0 - beat * 50.0) as u8;
                if front.loc.vpos > top.loc.vpos && front.is_lndone() {
                    style.render_longnote(d, 30.0, H-80.0, lnalpha);
                } else {
                    let mut nextbottom = None;
                    for ptr in front.upto(&top) {
                        let y = loc_to_y(&ptr.loc);
                        match ptr.data() {
                            LNStart(lane0,_) if lane0 == lane => {
                                assert!(nextbottom.is_none());
                                nextbottom = Some(y);
                            }
                            LNDone(lane0,_) if lane0 == lane => {
                                match nextbottom {
                                    Some(y2) => {
                                        style.render_longnote(d, y, y2, lnalpha);
                                        style.render_note(d, y2-5.0, y2);
                                        style.render_note(d, y-5.0, y);
                                    }
                                    None => {
                                        style.render_longnote(d, y, H-80.0, lnalpha);
                                        style.render_note(d, y-5.0, y);
                                    }
                                }
                                nextbottom = None;
                            }
                            Visible(lane0,_) if lane0 == lane => {
                                assert!(nextbottom.is_none());
                                style.render_note(d, y-5.0, y);
                            }
                            Bomb(lane0,_,_) if lane0 == lane => {
                                assert!(nextbottom.is_none());
                                style.render_bomb(d, y-5.0, y);
                            }
                            _ => {}
                        }
                    }

                    for &y in nextbottom.iter() {
                        style.render_longnote(d, 30.0, y, lnalpha);
                        style.render_note(d, y-5.0, y);
                    }
                }
            }
        });

        screen.draw_shaded_with_font(|d| {
            // render non-note objects (currently, measure bars)
            for ptr in bottom.upto(&top) {
                match ptr.data() {
                    MeasureBar => {
                        let y = loc_to_y(&ptr.loc);
                        d.rect(0.0, y, self.leftmost as f32, y + 1.0, RGB(0xc0,0xc0,0xc0));
                        for &rightmost in self.rightmost.iter() {
                            d.rect(rightmost as f32, y, W, y + 1.0, RGB(0xc0,0xc0,0xc0));
                        }
                    }
                    _ => {}
                }
            }

            // render grading line
            d.rect(0.0, H-85.0, self.leftmost as f32, H-80.0, RGBA(0xff,0,0,0x40));
            for &rightmost in self.rightmost.iter() {
                d.rect(rightmost as f32, H-85.0, W, H-80.0, RGBA(0xff,0,0,0x40));
            }

            // render grading text
            if self.gradelimit.is_some() && self.player.lastgrade.is_some() {
                let gradelimit = self.gradelimit.unwrap();
                let (lastgrade,_) = self.player.lastgrade.unwrap();
                let (gradename,gradecolor) = GRADES[lastgrade as uint];
                let delta = (cmp::max(gradelimit - self.player.now, 400) as f32 - 400.0) / 15.0;
                let cx = (self.leftmost / 2) as f32; // avoids half-pixels
                let cy = H / 2.0 - delta; // offseted center
                d.string(cx, cy - 40.0, 2.0, Alignment::Center, gradename, gradecolor);
                if self.player.lastcombo > 1 {
                    d.string(cx, cy - 12.0, 1.0, Alignment::Center,
                             format!("{} COMBO", self.player.lastcombo)[],
                             Gradient { zero: RGB(0xff,0xff,0xff), one: RGB(0x80,0x80,0x80) });
                }
                if self.player.opts.is_autoplay() {
                    d.string(cx, cy + 2.0, 1.0, Alignment::Center, "(AUTO)",
                             Gradient { zero: RGB(0xc0,0xc0,0xc0), one: RGB(0x40,0x40,0x40) });
                }
            }
        });

        screen.draw_textured(&self.sprite, |d| {
            // restore panel from the sprite
            d.rect_area(0.0, 0.0, W, 30.0, 0.0, 0.0, W, 30.0);
            d.rect_area(0.0, H-80.0, W, H, 0.0, H-80.0, W, H);
        });
        screen.draw_shaded_with_font(|d| {
            let elapsed = (self.player.now - self.player.origintime) / 1000;
            let duration = self.player.duration as uint;
            let durationmsec = (self.player.duration * 1000.0) as uint;

            // render panel text
            let black = RGB(0,0,0);
            d.string(10.0, 8.0, 1.0, Alignment::Left,
                     format!("SCORE {:07}", self.player.score)[], black);
            let nominalplayspeed = self.player.nominal_playspeed();
            d.string(5.0, H-78.0, 2.0, Alignment::Left,
                     format!("{:4.1}x", nominalplayspeed)[], black);
            d.string((self.leftmost-94) as f32, H-35.0, 1.0, Alignment::Left,
                     format!("{:02}:{:02} / {:02}:{:02}",
                             elapsed/60, elapsed%60, duration/60, duration%60)[], black);
            d.string(95.0, H-62.0, 1.0, Alignment::Left,
                     format!("@{:9.4}", self.player.cur.loc.vpos)[], black);
            d.string(95.0, H-78.0, 1.0, Alignment::Left,
                     format!("BPM {:6.2}", *self.player.bpm)[], black);
            let timetick = cmp::min(self.leftmost, (self.player.now - self.player.origintime) *
                                                   self.leftmost / durationmsec);
            d.glyph(6.0 + timetick as f32, H-52.0, 1.0, 95, RGB(0x40,0x40,0x40));

            // render gauge
            if !self.player.opts.is_autoplay() {
                // draw the gauge bar
                let gray = RGB(0x40,0x40,0x40);
                d.rect(0.0, H-16.0, 368.0, H, gray);
                d.rect(4.0, H-12.0, 360.0, H-4.0, black);

                // cycles four times per measure, [0,40)
                let width = if self.player.gauge < 0 {0}
                            else {self.player.gauge * 400 / MAXGAUGE - (beat * 40.0) as int};
                let width = cmp::min(cmp::max(width, 5), 360);
                let color = if self.player.gauge >= self.player.survival {RGB(0xc0,0,0)}
                            else {RGB(0xc0 - (beat * 160.0) as u8, 0, 0)};
                d.rect(4.0, H-12.0, 4.0 + width as f32, H-4.0, color);
            }
        });

        screen.swap_buffers();
    }

    fn deactivate(&mut self) {}

    fn consume(self: Box<PlayingScene>) -> Box<Scene+'static> {
        let scene = *self;
        let PlayingScene { screen, player, .. } = scene;
        PlayResultScene::new(screen, player) as Box<Scene+'static>
    }
}

