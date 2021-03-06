// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, 2014, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! Play result screen. Only used when the graphical `PlayingScene` finishes.

use std::rc::Rc;
use std::cell::RefCell;

use sdl::event;
use sdl::event::Event;
use gfx::screen::Screen;
use gfx::skin::render::Renderer;
use engine::player::Player;
use ui::scene::{Scene, SceneOptions, SceneCommand};

/// Play result scene.
pub struct PlayResultScene {
    /// Display screen.
    pub screen: Rc<RefCell<Screen>>,
    /// Game play state after playing.
    pub player: Player,
    /// Skin renderer.
    pub skin: RefCell<Renderer>,
}

impl PlayResultScene {
    /// Creates a new play result scene from the game play state after `PlayingScene`.
    pub fn new(screen: Rc<RefCell<Screen>>, player: Player) -> Box<PlayResultScene> {
        let skin = match player.opts.load_skin("playresult.cson") {
            Ok(skin) => skin,
            Err(err) => die!("{}", err),
        };
        box PlayResultScene { screen: screen, player: player,
                              skin: RefCell::new(Renderer::new(skin)) }
    }
}

impl Scene for PlayResultScene {
    fn activate(&mut self) -> SceneCommand { SceneCommand::Continue }

    fn scene_options(&self) -> SceneOptions { SceneOptions::new().tpslimit(20).fpslimit(1) }

    fn tick(&mut self) -> SceneCommand {
        loop {
            match event::poll_event() {
                Event::Key(event::Key::Escape,true,_,_) |
                Event::Key(event::Key::Return,true,_,_) => { return SceneCommand::Pop; }
                Event::Quit => { return SceneCommand::Exit; }
                Event::None => { break; }
                _ => {}
            }
        }
        SceneCommand::Continue
    }

    fn render(&self) {
        let mut screen = self.screen.borrow_mut();

        screen.clear();
        self.skin.borrow_mut().render(screen.deref_mut(), self);
        screen.swap_buffers();
    }

    fn deactivate(&mut self) {}

    fn consume(self: Box<PlayResultScene>) -> Box<Scene+'static> { panic!("unreachable"); }
}

define_hooks! {
    for PlayResultScene |scene, id, parent, body| {
        delegate scene.player;
    }
}

