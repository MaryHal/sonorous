// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! Initialization.

use std::rc::Rc;
use std::cell::RefCell;

use sdl::*;
use sdl_image;
use sdl_mixer;
use engine::resource::{BGAW, BGAH, SAMPLERATE};
use gfx::screen::Screen;

/// The width of screen, unless the exclusive mode.
pub static SCREENW: uint = 800;
/// The height of screen, unless the exclusive mode.
pub static SCREENH: uint = 600;

/// Initializes SDL video subsystem, and creates a small screen for BGAs (`BGAW` by `BGAH` pixels)
/// if `exclusive` is set, or a full-sized screen (`SCREENW` by `SCREENH` pixels) otherwise.
/// `fullscreen` is ignored when `exclusive` is set.
pub fn init_video(exclusive: bool, fullscreen: bool) -> Rc<RefCell<Screen>> {
    if !init([InitVideo]) {
        die!("SDL Initialization Failure: {}", get_error());
    }
    sdl_image::init([sdl_image::InitJPG, sdl_image::InitPNG]);

    let (width, height, fullscreen) = if exclusive {
        (BGAW, BGAH, false)
    } else {
        (SCREENW, SCREENH, fullscreen)
    };
    let screen = match Screen::new(width, height, fullscreen) {
        Ok(screen) => screen,
        Err(err) => die!("Failed to initialize screen: {}", err)
    };
    if !exclusive {
        mouse::set_cursor_visible(false);
    }

    wm::set_caption(::version(), "");
    Rc::new(RefCell::new(screen))
}

/// Initializes SDL audio subsystem and SDL_mixer.
pub fn init_audio() {
    if !init([InitAudio]) {
        die!("SDL Initialization Failure: {}", get_error());
    }
    //sdl_mixer::init([sdl_mixer::InitOGG, sdl_mixer::InitMP3]); // TODO
    if sdl_mixer::open(SAMPLERATE, audio::S16_AUDIO_FORMAT, audio::Stereo, 2048).is_err() {
        die!("SDL Mixer Initialization Failure");
    }
}

/// Initializes a joystick with given index.
pub fn init_joystick(joyidx: uint) -> ~joy::Joystick {
    if !init([InitJoystick]) {
        die!("SDL Initialization Failure: {}", get_error());
    }
    unsafe {
        joy::ll::SDL_JoystickEventState(1); // TODO rust-sdl patch
    }
    match joy::Joystick::open(joyidx as int) {
        Ok(joy) => joy,
        Err(err) => die!("SDL Joystick Initialization Failure: {}", err)
    }
}

