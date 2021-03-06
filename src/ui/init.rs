// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, 2014, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! Initialization.

use sdl;
use sdl_image;
use sdl_mixer;
use engine::resource::{BGAW, BGAH, SAMPLERATE};
use gfx::screen::Screen;

/// The width of screen, unless the exclusive mode.
pub const SCREENW: uint = 800;
/// The height of screen, unless the exclusive mode.
pub const SCREENH: uint = 600;

/// Initializes SDL video subsystem, and creates a small screen for BGAs (`BGAW` by `BGAH` pixels)
/// if `exclusive` is set, or a full-sized screen (`SCREENW` by `SCREENH` pixels) otherwise.
/// `fullscreen` is ignored when `exclusive` is set.
pub fn init_video(exclusive: bool, fullscreen: bool) -> Screen {
    if !sdl::init([sdl::InitFlag::Video][]) {
        die!("SDL Initialization Failure: {}", sdl::get_error());
    }
    sdl_image::init([sdl_image::InitFlag::JPG, sdl_image::InitFlag::PNG][]);

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
        sdl::mouse::set_cursor_visible(false);
    }

    sdl::wm::set_caption(::version()[], "");
    screen
}

/// Initializes SDL audio subsystem and SDL_mixer.
pub fn init_audio() {
    if !sdl::init([sdl::InitFlag::Audio][]) {
        die!("SDL Initialization Failure: {}", sdl::get_error());
    }
    //sdl_mixer::init([sdl_mixer::InitFlag::OGG, sdl_mixer::InitFlag::MP3][]); // TODO
    if sdl_mixer::open(SAMPLERATE, sdl::audio::S16_AUDIO_FORMAT,
                       sdl::audio::Channels::Stereo, 2048).is_err() {
        die!("SDL Mixer Initialization Failure");
    }
}

/// Initializes a joystick with given index.
pub fn init_joystick(joyidx: uint) -> sdl::joy::Joystick {
    if !sdl::init([sdl::InitFlag::Joystick][]) {
        die!("SDL Initialization Failure: {}", sdl::get_error());
    }
    unsafe {
        sdl::joy::ll::SDL_JoystickEventState(1); // TODO rust-sdl patch
    }
    match sdl::joy::Joystick::open(joyidx as int) {
        Ok(joy) => joy,
        Err(err) => die!("SDL Joystick Initialization Failure: {}", err)
    }
}

