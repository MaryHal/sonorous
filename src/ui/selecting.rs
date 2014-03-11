// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, 2014, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! Song and pattern selection screen.

use std::{str, cmp, io, os, comm, task};
use std::rc::Rc;
use std::cell::RefCell;
use std::rand::{rng, Rng};
use std::comm::{Chan, Port};
use sync::RWArc;

use sdl::{event, get_ticks};
use format::timeline::TimelineInfo;
use format::bms;
use format::bms::Bms;
use util::filesearch::SearchContext;
use util::envelope::Envelope;
use util::md5::{MD5, ToHex};
use gfx::gl::{PreparedSurface, Texture2D};
use gfx::draw::{ShadedDrawingTraits, TexturedDrawingTraits};
use gfx::screen::Screen;
use engine::input::read_keymap;
use engine::keyspec::{KeySpec, key_spec};
use engine::resource::{SearchContextAdditions, LoadedImageResource, LoadedImage};
use engine::player::apply_modf;
use engine::skin::ast::Skin;
use engine::skin::hook::{Scalar, AsScalar, IntoScalar, Hook};
use engine::skin::render::Renderer;
use ui::scene::{Scene, SceneOptions, SceneCommand, Continue, PushScene, PopScene, Exit};
use ui::options::Options;
use ui::loading::LoadingScene;

/// The BMS data that has been preprocessed for modifiers and analyzed but yet to be loaded.
pub struct PreprocessedBms {
    /// Yet-to-be-loaded BMS data.
    bms: Bms,
    /// The derived timeline information.
    infos: TimelineInfo,
    /// The key specification.
    keyspec: ~KeySpec,
}

/// Loads and preprocesses the BMS file from given options. Frontend routines should use this.
pub fn preprocess_bms<'r,R:Rng>(
        bmspath: &Path, f: &mut Reader, opts: &Options, r: &mut R,
        loaderopts: &bms::load::LoaderOptions, callback: bms::load::Callback<'r>)
                                -> Result<~PreprocessedBms,~str> {
    let bms = try!(bms::load::load_bms(f, r, loaderopts, callback));
    let mut bms = bms.with_bmspath(bmspath);
    let keyspec = try!(key_spec(&bms, opts.preset.clone(),
                                opts.leftkeys.clone(), opts.rightkeys.clone()));
    keyspec.filter_timeline(&mut bms.timeline);
    let infos = bms.timeline.analyze();
    for &modf in opts.modf.iter() {
        apply_modf(&mut bms, modf, r, keyspec);
    }
    Ok(~PreprocessedBms { bms: bms, infos: infos, keyspec: keyspec })
}

/// Internal message from the worker task to the main task.
enum Message {
    /// The worker has scanned more files.
    PushFiles(~[Path]),
    /// The worker has finished scanning files.
    NoMoreFiles,
    /// The worker has failed to read and/or load the BMS file. Error message follows.
    BmsFailed(Path,~str),
    /// The worker has read the BMS file and calculated its MD5 hash.
    BmsHashRead(Path,[u8, ..16]),
    /// The worker has loaded the BMS file or failed to do so. Since this message can be delayed,
    /// the main task should ignore the message with non-current paths.
    BmsLoaded(Path,~PreprocessedBms,~[(Option<uint>,bms::diag::BmsMessage)]),
    /// The worker has loaded the banner image (the second `~str`) for the BMS file (the first
    /// `Path`). This may be sent after `BmsLoaded` message. Due to the same reason as above
    /// the main task should ignore the message with non-current paths.
    BmsBannerLoaded(Path,~str,Envelope<PreparedSurface>),
}

/// Preloaded game data.
pub struct PreloadedData {
    /// A part of the game data necessary for initializing `Player`. Anything else is used for
    /// the selection screen only.
    preproc: ~PreprocessedBms,
    /// A banner texture, if any.
    banner: Option<Rc<Texture2D>>,
    /// A tentatively calculated duration of the BMS file in seconds. This is tentative because
    /// we don't know lengths of key sounds and BGMs yet.
    duration: f64,
    /// A list of diagnostic messages returned during the loading.
    messages: ~[(Option<uint>,bms::diag::BmsMessage)],
}

impl PreloadedData {
    /// Creates and calculates some necessary informations from given processed BMS and messages.
    pub fn new(preproc: ~PreprocessedBms,
               messages: ~[(Option<uint>,bms::diag::BmsMessage)]) -> PreloadedData {
        let originoffset = preproc.infos.originoffset;
        let duration = preproc.bms.timeline.duration(originoffset, |_| 0.0);
        PreloadedData { preproc: preproc, banner: None, duration: duration, messages: messages }
    }
}

/// The state of preloaded game data.
pub enum PreloadingState {
    /// The selected entry cannot be used for preloading or the entry does not exist.
    NothingToPreload,
    /// The selected entry should be preloaded after given timestamp (as per `sdl::get_ticks()`).
    PreloadAfter(uint),
    /// Preloading is in progress.
    Preloading,
    /// Preloading finished with a success.
    Preloaded(PreloadedData),
    /// Preloading finished with a failure. Error message is available.
    PreloadFailed(~str),
}

/// The scanned entry.
pub struct Entry {
    /// A path to loaded file.
    path: Path,
    /// MD5 hash. Only present when it has been read.
    hash: Option<[u8, ..16]>,
}

/// Song/pattern selection scene context. Used when the directory path is specified.
pub struct SelectingScene {
    /// Display screen.
    screen: Rc<RefCell<Screen>>,
    /// Game play options.
    opts: Rc<Options>,
    /// Parsed skin data.
    skin: Rc<Skin>,

    /// The root path of the scanner.
    root: Path,
    /// A list of scanned entries.
    files: ~[Entry],
    /// Set to true when the scanner finished scanning.
    filesdone: bool,
    /// The index of the topmost entry visible on the screen.
    scrolloffset: uint,
    /// The index of the selected entry.
    offset: uint,
    /// Preloaded game data or preloading state if any.
    preloaded: PreloadingState,
    /// A port for receiving worker messages.
    priv port: Port<Message>,
    /// A shared channel to which workers send messages.
    priv chan: Chan<Message>,
    /// A shared cell, set to false when the scene is deactivated.
    priv keepgoing: RWArc<bool>,
}

/// Returns true if the path should be parsed as a BMS file.
fn is_bms_file(path: &Path) -> bool {
    use std::ascii::StrAsciiExt;
    match path.extension().and_then(str::from_utf8) {
        Some(ext) => match ext.to_ascii_lower().as_slice() {
            "bms" | "bme" | "bml" | "pms" => true,
            _ => false
        },
        _ => false
    }
}

/// Spawns an worker task. We are required to use `libnative` due to the SDL event loop.
/// Also we need to use our own wrapper to avoid "sending on a closed channel" error from
/// the default `future_result` wrapper.
fn spawn_worker_task(name: ~str, body: proc(), on_error: proc()) {
    task::task().named(name + " wrapper").spawn(proc() {
        let ret = task::task().named(name).try(body);
        if ret.is_err() { on_error(); }
    });
}

/// Prints a diagnostic message to the screen.
/// This can be directly used as a parser message callback.
pub fn print_diag(line: Option<uint>, msg: bms::diag::BmsMessage) -> bool {
    let atline = match line {
        Some(line) => format!(" at line {}", line),
        None => ~"",
    };
    warn!("[{}{}] {}", msg.severity, atline, msg);
    true
}

/// The maximum number of entries displayed in one screen.
static NUMENTRIES: uint = 15;
/// The number of milliseconds after `update_offset` before the actual preloading starts,
/// in order to avoid the worker congestion.
static PRELOAD_DELAY: uint = 300;

impl SelectingScene {
    /// Creates a new selection scene from the screen, the root path and initial options.
    pub fn new(screen: Rc<RefCell<Screen>>, root: &Path, opts: Rc<Options>) -> ~SelectingScene {
        let skin = match opts.borrow().load_skin("selecting.json") {
            Ok(skin) => skin,
            Err(err) => die!("{}", err),
        };
        let root = os::make_absolute(root);
        let (port, chan) = Chan::new();
        ~SelectingScene {
            screen: screen, opts: opts, skin: Rc::new(skin), root: root,
            files: ~[], filesdone: false, scrolloffset: 0, offset: 0, preloaded: NothingToPreload,
            port: port, chan: chan, keepgoing: RWArc::new(true),
        }
    }

    /// Spawns a new scanning task.
    pub fn spawn_scanning_task(&self) {
        let root = self.root.clone();
        let chan = self.chan.clone();
        let chan_ = self.chan.clone();
        let keepgoing = self.keepgoing.clone();
        spawn_worker_task(~"scanner", proc() {
            fn recur(search: &mut SearchContext, root: Path,
                     chan: &Chan<Message>, keepgoing: &RWArc<bool>) -> bool {
                if !keepgoing.read(|v| *v) { return false; }

                let (dirs, files) = {
                    let (dirs, files) = search.get_entries(&root);
                    (dirs.to_owned(), files.iter().map(|e| e.clone()).filter(is_bms_file).collect())
                }; // so that we can reborrow `search`
                chan.try_send(PushFiles(files));
                for dir in dirs.move_iter() {
                    if !recur(search, dir, chan, keepgoing) { return false; }
                }
                true
            }
            let mut search = SearchContext::new();
            if recur(&mut search, root.clone(), &chan, &keepgoing) {
                chan.try_send(NoMoreFiles);
            }
        }, proc() {
            chan_.try_send(NoMoreFiles);
        });
    }

    /// Clears the current scanned files and restarts the scanning task.
    pub fn refresh(&mut self) {
        // terminates prior tasks
        assert!(self.keepgoing.read(|&v| v));
        self.keepgoing.write(|v| { *v = false; });
        self.keepgoing = RWArc::new(true);
        let (port, chan) = Chan::new();
        self.port = port;
        self.chan = chan;

        self.files.clear();
        self.filesdone = false;
        self.scrolloffset = 0;
        self.offset = 0;
        self.preloaded = NothingToPreload;
        self.spawn_scanning_task();
    }

    /// Spawns a new preloading task.
    pub fn spawn_preloading_task(&self, path: &Path) {
        let bmspath = path.clone();
        let bmspath_ = path.clone();
        let opts = self.opts.borrow().clone();
        let chan = self.chan.clone();
        let chan_ = self.chan.clone();
        spawn_worker_task(~"preloader", proc() {
            let opts = Rc::new(opts);

            let load_banner = |bannerpath: ~str, basepath: Option<Path>| {
                let mut search = SearchContext::new();
                let basedir = basepath.clone().unwrap_or(Path::new("."));
                let fullpath = search.resolve_relative_path_for_image(bannerpath, &basedir);
                let res = fullpath.and_then(|path| LoadedImageResource::new(&path, false));
                match res {
                    Ok(LoadedImage(surface)) => {
                        chan.try_send(BmsBannerLoaded(bmspath.clone(), bannerpath,
                                                      Envelope::new(surface)));
                    }
                    _ => {}
                }
            };

            let load_with_reader = |mut f| {
                let mut r = rng();
                let mut diags = ~[];
                let loaderopts = opts.borrow().loader_options();

                let preproc = {
                    let callback = |line: Option<uint>, msg: bms::diag::BmsMessage| {
                        diags.push((line, msg));
                        true
                    };
                    preprocess_bms(&bmspath, &mut f, opts.borrow(), &mut r, &loaderopts, callback)
                };
                match preproc {
                    Ok(preproc) => {
                        let banner = preproc.bms.meta.banner.clone();
                        let basepath = preproc.bms.meta.basepath.clone();
                        chan.try_send(BmsLoaded(bmspath.clone(), preproc, diags));
                        if banner.is_some() {
                            load_banner(banner.unwrap(), basepath);
                        }
                    }
                    Err(err) => {
                        chan.try_send(BmsFailed(bmspath.clone(), err));
                    }
                }
            };

            match io::File::open(&bmspath) {
                Ok(mut f) => {
                    let buf = f.read_to_end().ok().unwrap_or_else(|| ~[]);
                    let hash = MD5::from_buffer(buf).final();
                    chan.try_send(BmsHashRead(bmspath.clone(), hash));

                    // since we have read the file we don't want the parser to read it again.
                    load_with_reader(io::MemReader::new(buf));
                }
                Err(err) => {
                    chan.try_send(BmsFailed(bmspath.clone(), err.to_str()));
                }
            }
        }, proc() {
            // the task failed to send the error message, so the wrapper sends it instead
            chan_.try_send(BmsFailed(bmspath_, ~"unexpected error"));
        });
    }

    /// Returns a `Path` to the currently selected entry if any.
    pub fn current<'r>(&'r self) -> Option<&'r Path> {
        if self.offset < self.files.len() {
            Some(&self.files[self.offset].path)
        } else {
            None
        }
    }

    /// Checks if a given `Path` indeed points to the current entry.
    pub fn is_current(&self, path: &Path) -> bool {
        match self.current() { Some(current) => current == path, None => false }
    }

    /// Updates the selected entry. `offset` may be out of the range.
    pub fn update_offset(&mut self, offset: uint) {
        self.offset = offset;
        if offset < self.files.len() {
            self.preloaded = PreloadAfter(get_ticks() + PRELOAD_DELAY);
        } else {
            self.preloaded = NothingToPreload;
        }

        if self.scrolloffset > offset {
            self.scrolloffset = offset;
        } else if self.scrolloffset + (NUMENTRIES-1) < offset {
            self.scrolloffset = offset - (NUMENTRIES-1);
        }
    }

    /// Creates a new `LoadingScene` from the currently selected entry. It will load the BMS file
    /// or use the preloaded data if possible.
    pub fn create_loading_scene(&mut self) -> Option<~Scene:> {
        use std::mem::replace;

        let preloaded = replace(&mut self.preloaded, PreloadAfter(0));
        let PreprocessedBms { bms, infos, keyspec } =
            match preloaded {
                Preloaded(data) => *data.preproc, // use the preloaded data if possible
                _ => {
                    let path = match self.current() {
                        Some(path) => path,
                        None => { return None; }
                    };
                    let mut f = match io::File::open(path) {
                        Ok(f) => f,
                        Err(err) => {
                            warn!("Failed to open {}: {}", path.display(), err);
                            return None;
                        }
                    };
                    let mut r = rng();
                    let opts = self.opts.borrow();
                    let ret = preprocess_bms(path, &mut f as &mut Reader, opts, &mut r,
                                             &opts.loader_options(), print_diag);
                    match ret {
                        Ok(preproc) => *preproc,
                        Err(err) => { warn!("{}", err); return None; }
                    }
                }
            };
        let keymap = match read_keymap(keyspec, os::getenv) {
            Ok(map) => ~map,
            Err(err) => die!("{}", err)
        };
        Some(LoadingScene::new(self.screen.clone(), bms, infos,
                               keyspec, keymap, self.opts.clone()) as ~Scene:)
    }
}

impl Scene for SelectingScene {
    fn activate(&mut self) -> SceneCommand {
        self.keepgoing.write(|v| { *v = true; });
        if !self.filesdone { self.refresh(); }
        event::enable_key_repeat(event::DefaultRepeatDelay, event::DefaultRepeatInterval);
        Continue
    }

    fn scene_options(&self) -> SceneOptions { SceneOptions::new().tpslimit(50).fpslimit(20) }

    fn tick(&mut self) -> SceneCommand {
        loop {
            match event::poll_event() {
                event::KeyEvent(event::EscapeKey,true,_,_) => { return PopScene; }
                event::QuitEvent => { return Exit; }
                event::NoEvent => { break; },

                // navigation
                event::KeyEvent(event::UpKey,true,_,_) => {
                    if self.offset > 0 {
                        self.update_offset(self.offset - 1);
                    }
                }
                event::KeyEvent(event::DownKey,true,_,_) => {
                    if self.offset + 1 < self.files.len() {
                        self.update_offset(self.offset + 1);
                    }
                }
                event::KeyEvent(event::PageUpKey,true,_,_) => {
                    if self.offset > (NUMENTRIES-1) - 1 {
                        self.update_offset(self.offset - (NUMENTRIES-1));
                    } else if self.offset > 0 {
                        self.update_offset(0);
                    }
                }
                event::KeyEvent(event::PageDownKey,true,_,_) => {
                    let nfiles = self.files.len();
                    if self.offset + (NUMENTRIES-1) < nfiles {
                        self.update_offset(self.offset + (NUMENTRIES-1));
                    } else if self.offset + 1 < nfiles {
                        self.update_offset(nfiles - 1);
                    }
                }
                event::KeyEvent(event::HomeKey,true,_,_) => {
                    if self.offset > 0 {
                        self.update_offset(0);
                    }
                }
                event::KeyEvent(event::EndKey,true,_,_) => {
                    let nfiles = self.files.len();
                    if self.offset + 1 < nfiles {
                        self.update_offset(nfiles - 1);
                    }
                }

                // refresh
                event::KeyEvent(event::F5Key,true,_,_) => {
                    self.refresh();
                }

                // (auto)play
                event::KeyEvent(event::ReturnKey,true,_,_) => {
                    match self.create_loading_scene() {
                        Some(scene) => { return PushScene(scene); }
                        None => {}
                    }
                }

                _ => {}
            }
        }

        loop {
            match self.port.try_recv() {
                comm::Empty | comm::Disconnected => { break; }

                comm::Data(PushFiles(paths)) => {
                    if self.files.is_empty() { // immediately preloads the first entry
                        self.preloaded = PreloadAfter(0);
                    }
                    for path in paths.move_iter() {
                        self.files.push(Entry { path: path, hash: None });
                    }
                }
                comm::Data(NoMoreFiles) => {
                    self.filesdone = true;
                }
                comm::Data(BmsFailed(bmspath,err)) => {
                    if !self.is_current(&bmspath) { continue; }
                    self.preloaded = PreloadFailed(err);
                }
                comm::Data(BmsHashRead(bmspath,hash)) => {
                    if !self.is_current(&bmspath) { continue; }
                    self.files[self.offset].hash = Some(hash);
                }
                comm::Data(BmsLoaded(bmspath,preproc,messages)) => {
                    if !self.is_current(&bmspath) { continue; }
                    self.preloaded = Preloaded(PreloadedData::new(preproc, messages));
                }
                comm::Data(BmsBannerLoaded(bmspath,imgpath,prepared)) => {
                    if !self.is_current(&bmspath) { continue; }
                    match self.preloaded {
                        Preloaded(ref mut data) if data.banner.is_none() => {
                            let prepared = prepared.unwrap();
                            match Texture2D::from_prepared_surface(&prepared, false, false) {
                                Ok(tex) => { data.banner = Some(Rc::new(tex)); }
                                Err(_err) => { warn!("failed to load image \\#BANNER ({})",
                                                     imgpath.to_str()); }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if self.offset < self.files.len() {
            match self.preloaded {
                PreloadAfter(timeout) if timeout < get_ticks() => {
                    // preload the current entry after some delay
                    self.preloaded = match self.current() {
                        Some(path) => { self.spawn_preloading_task(path); Preloading },
                        None => NothingToPreload, // XXX wait what happened?!
                    };
                }
                _ => {}
            }
        }

        Continue
    }

    fn render(&self) {
        let screen__ = self.screen.borrow();
        let mut screen_ = screen__.borrow_mut();
        let screen = screen_.get();

        screen.clear();

        // TODO should move this to the skin too
        screen.draw_shaded_with_font(|d| {
            use gfx::color::RGB;
            let top = cmp::min(self.scrolloffset, self.files.len());
            let pixelsperslot =
                (NUMENTRIES as f32 * 20.0 - 4.0) / cmp::max(NUMENTRIES, self.files.len()) as f32;
            d.rect(2.0, 2.0 + top as f32 * pixelsperslot,
                   7.0, 2.0 + (top + NUMENTRIES) as f32 * pixelsperslot, RGB(0xc0,0xc0,0xc0));
        });

        Renderer::render(screen, self.skin.clone().borrow(), self);

        screen.swap_buffers();
    }

    fn deactivate(&mut self) {
        event::enable_key_repeat(event::CustomRepeatDelay(0), event::DefaultRepeatInterval);
        self.keepgoing.write(|v| { *v = false; });
    }

    fn consume(~self) -> ~Scene: { fail!("unreachable"); }
}

////////////////////////////////////////////////////////////////////////
// hooks

impl Hook for PreprocessedBms {
    fn scalar_hook<'a>(&'a self, id: &str) -> Option<Scalar<'a>> {
        self.bms.scalar_hook(id)
            .or_else(|| self.infos.scalar_hook(id))
            .or_else(|| self.keyspec.scalar_hook(id))
    }

    fn block_hook(&self, id: &str, parent: &Hook, body: |&Hook, &str| -> bool) -> bool {
        self.bms.run_block_hook(id, parent, &body) ||
            self.infos.run_block_hook(id, parent, &body) ||
            self.keyspec.run_block_hook(id, parent, &body)
    }
}

impl Hook for (Option<uint>, bms::diag::BmsMessage) {
    fn scalar_hook<'a>(&'a self, id: &str) -> Option<Scalar<'a>> {
        let (lineno, ref msg) = *self;
        match id {
            "msg.line" => lineno.map(|v| v.into_scalar()),
            "msg.text" => Some(msg.message.into_scalar()),
            _ => None,
        }
    }

    fn block_hook(&self, id: &str, parent: &Hook, body: |&Hook, &str| -> bool) -> bool {
        let (lineno, ref msg) = *self;
        match id {
            "msg" => { body(parent, msg.id); }
            "msg.line" => { lineno.is_some() && body(parent, ""); }
            "msg.severity" => match msg.severity {
                bms::diag::Note    => { body(parent, "note"); }
                bms::diag::Warning => { body(parent, "warning"); }
                bms::diag::Fatal   => { body(parent, "fatal"); }
            },
            _ => { return false; }
        }
        true
    }
}

impl Hook for PreloadedData {
    fn scalar_hook<'a>(&'a self, id: &str) -> Option<Scalar<'a>> {
        match id {
            "meta.duration" => Some(self.duration.into_scalar()),
            "meta.banner" => match self.banner {
                Some(ref tex) => Some(tex.as_scalar()),
                None => None,
            },
            _ => self.preproc.scalar_hook(id)
        }
    }

    fn block_hook(&self, id: &str, parent: &Hook, body: |&Hook, &str| -> bool) -> bool {
        match id {
            "messages" => {
                // TODO 20 messages limit is arbitrary, should really be handled by skin
                self.messages.iter().take(20).advance(|msg| body(&parent.delegate(msg), ""));
            }
            "meta.banner" => { self.banner.is_some() && body(parent, ""); }
            _ => { return self.preproc.run_block_hook(id, parent, &body); }
        }
        true
    }
}

impl Hook for SelectingScene {
    fn block_hook(&self, id: &str, parent: &Hook, body: |&Hook, &str| -> bool) -> bool {
        match id {
            "autoplay" => { self.opts.borrow().is_autoplay() && body(parent, ""); }
            "scanning" => { self.filesdone || body(parent, ""); }
            "entries" => {
                let top = cmp::min(self.scrolloffset, self.files.len());
                let bottom = cmp::min(self.scrolloffset + NUMENTRIES, self.files.len());
                for (i, entry) in self.files.slice(top, bottom).iter().enumerate() {
                    let inverted = self.scrolloffset + i == self.offset;
                    if !body(&(self, inverted, entry), "") { break; }
                }
            }
            "preload" => match self.preloaded {
                NothingToPreload | PreloadAfter(..) => {}
                Preloading => { body(parent, "loading"); }
                Preloaded(ref data) => { body(&parent.delegate(data), "loaded"); }
                PreloadFailed(ref err) => {
                    body(&parent.add_text("preload.error", *err), "failed");
                }
            },
            _ => { return false; }
        }
        true
    }
}

impl<'a> Hook for (&'a SelectingScene, bool, &'a Entry) {
    fn scalar_hook<'a>(&'a self, id: &str) -> Option<Scalar<'a>> {
        let (scene, _inverted, entry) = *self;
        match id {
            "entry.path" => {
                let path = entry.path.path_relative_from(&scene.root)
                                     .unwrap_or_else(|| entry.path.clone());
                Some(path.display().to_str().into_scalar())
            },
            "entry.hash" => entry.hash.map(|h| h.to_hex().into_scalar()),
            _ => scene.scalar_hook(id)
        }
    }

    fn block_hook(&self, id: &str, parent: &Hook, body: |&Hook, &str| -> bool) -> bool {
        let (scene, inverted, entry) = *self;
        match id {
            "entry.hash" => { entry.hash.is_some() && body(parent, ""); }
            "entry.inverted" => { inverted && body(parent, ""); }
            _ => { return scene.run_block_hook(id, parent, &body); }
        }
        true
    }
}

