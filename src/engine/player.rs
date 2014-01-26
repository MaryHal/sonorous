// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! Core game play logics. Handles the input (if any) and sound but not the user interface.

use std::{vec, cmp, num};
use std::rand::Rng;
use std::rc::Rc;

use sdl::{get_ticks, event};
use ext::sdl_mixer;
use format::obj::*;
use format::timeline::TimelineInfo;
use format::pointer::*;
use format::bms::*;
use engine::keyspec::*;
use engine::input::{Input, KeyInput, JoyAxisInput, JoyButtonInput, QuitInput};
use engine::input::{LaneInput, SpeedUpInput, SpeedDownInput};
use engine::input::{InputState, Positive, Neutral, Negative};
use engine::input::{KeyMap};
use engine::resource::SoundResource;
use ui::options::*;

/// Applies given modifier to given group of lanes in the game data. `begin` and `end` should be
/// a valid range from 0 to `keyspec.order.len()`, where `end` is exclusive.
pub fn apply_modf_to_lanes<R:Rng>(timeline: &mut BmsTimeline, modf: Modf, r: &mut R,
                                  keyspec: &KeySpec, begin: uint, end: uint) {
    use timeline_modf = format::timeline::modf;

    let mut lanes = ~[];
    for i in range(begin, end) {
        let lane = keyspec.order[i];
        let kind = keyspec.kinds[lane.to_uint()];
        if modf == ShuffleExModf || modf == RandomExModf ||
                kind.map_or(false, |kind| kind.counts_as_key()) {
            lanes.push(lane);
        }
    }

    match modf {
        MirrorModf => timeline_modf::mirror(timeline, lanes),
        ShuffleModf | ShuffleExModf => timeline_modf::shuffle(timeline, r, lanes),
        RandomModf | RandomExModf => timeline_modf::randomize(timeline, r, lanes)
    };
}

/// Applies given modifier to the game data.
pub fn apply_modf<R:Rng>(bms: &mut Bms, modf: Modf, r: &mut R, keyspec: &KeySpec) {
    apply_modf_to_lanes(&mut bms.timeline, modf, r, keyspec, 0, keyspec.split);
    if keyspec.split < keyspec.order.len() {
        apply_modf_to_lanes(&mut bms.timeline, modf, r, keyspec,
                            keyspec.split, keyspec.order.len());
    }
}

/// A list of image references displayed in BGA layers (henceforth the BGA state). Not all image
/// referenced here is directly rendered, but the references themselves are kept.
pub type BGAState = [BGARef<ImageRef>, ..NLAYERS];

/// Returns the initial BGA state. Note that merely setting a particular layer doesn't start
/// the movie playback; `poorbgafix` in `parser::parse` function handles it.
pub fn initial_bga_state() -> BGAState {
    [BlankBGA, BlankBGA, BlankBGA, ImageBGA(ImageRef(Key(0)))]
}

/// Grades. Sonorous performs the time-based grading as long as possible (it can go wrong when
/// the object is near the discontinuity due to the current implementation strategy).
#[deriving(Eq)]
pub enum Grade {
    /**
     * Issued when the player did not input the object at all, the player was pressing the key
     * while a bomb passes through the corresponding lane, or failed to unpress the key within
     * the grading area for the end of LN. Resets the combo number, decreases the gauge
     * by severe amount (`MISS_DAMAGE` unless specified by the bomb) and displays the POOR BGA
     * for moments.
     *
     * Several games also use separate grading areas for empty lanes next to the object,
     * in order to avoid continuing the consecutive run ("combo") of acceptable grades by
     * just pressing every keys in the correct timing instead of pressing only lanes containing
     * objects. While this system is not a bad thing (and there are several BMS implementations
     * that use it), it is tricky to implement for the all situations. Sonorous currently
     * does not use this system due to the complexity.
     */
    MISS = 0,
    /// Issued when the player inputed the object and the normalized time difference (that is,
    /// the time difference multiplied by `Player::gradefactor`) between the input point and
    /// the object is between `GOOD_CUTOFF` and `BAD_CUTOFF` seconds. Resets the combo number,
    /// decreases the gauge by moderate amount (`BAD_DAMAGE`) and displays the POOR BGA for moments.
    BAD  = 1,
    /// Issued when the player inputed the object and the normalized time difference is between
    /// `GREAT_CUTOFF` and `GOOD_CUTOFF` seconds. Both the combo number and gauge is left unchanged.
    GOOD = 2,
    /// Issued when the player inputed the object and the normalized time difference is between
    /// `COOL_CUTOFF` and `GREAT_CUTOFF` . The combo number is increased by one and the gauge is
    /// replenished by small amount.
    GREAT = 3,
    /// Issued when the player inputed the object and the normalized time difference is less
    /// than `COOL_CUTOFF` . The combo number is increased by one and the gauge is replenished by
    /// large amount.
    COOL = 4,
}

/// Required time difference in seconds to get at least COOL grade.
static COOL_CUTOFF: f64 = 0.0144;
/// Required time difference in seconds to get at least GREAT grade.
static GREAT_CUTOFF: f64 = 0.048;
/// Required time difference in seconds to get at least GOOD grade.
static GOOD_CUTOFF: f64 = 0.084;
/// Required time difference in seconds to get at least BAD grade.
static BAD_CUTOFF: f64 = 0.144;

/// The number of available grades.
pub static NGRADES: uint = 5;

/// The maximum (internal) value for the gauge.
pub static MAXGAUGE: int = 512;
/// A base score per exact input. Actual score can increase by the combo (up to 2x) or decrease
/// by the larger time difference.
pub static SCOREPERNOTE: f64 = 300.0;

/// A damage due to the MISS grading. Only applied when the grading is not due to the bomb.
static MISS_DAMAGE: Damage = GaugeDamage(0.059);
/// A damage due to the BAD grading.
static BAD_DAMAGE: Damage = GaugeDamage(0.030);

/// Game play states independent to the display.
pub struct Player {
    /// The game play options.
    opts: Rc<Options>,
    /// The current BMS metadata.
    meta: BmsMeta,
    /// The current timeline. This is a managed pointer so that `Pointer` can be created for it.
    timeline: Rc<BmsTimeline>,
    /// The derived timeline information.
    infos: TimelineInfo,
    /// The length of BMS file in seconds as calculated by `bms_duration`.
    duration: f64,
    /// The key specification.
    keyspec: ~KeySpec,
    /// The input mapping.
    keymap: ~KeyMap,

    /// Set to true if the corresponding object in `bms.objs` had graded and should not be
    /// graded twice. Its length equals to that of `bms.objs`.
    nograding: ~[bool],
    /// Sound resources.
    sndres: ~[SoundResource],
    /// A sound chunk used for beeps. It always plays on the channel #0.
    beep: ~sdl_mixer::Chunk,
    /// Last channels in which the corresponding sound in `sndres` was played.
    sndlastch: ~[Option<uint>],
    /// Indices to last sounds which the channel has played. For every `x`, if `sndlastch[x] ==
    /// Some(y)` then `sndlastchmap[y] == Some(x)` and vice versa.
    lastchsnd: ~[Option<uint>],
    /// Currently active BGA layers.
    bga: BGAState,

    /// The chart expansion rate, or "play speed". One measure has the length of 400 pixels
    /// times the play speed, so higher play speed means that objects will fall much more
    /// quickly (hence the name).
    playspeed: f64,
    /// The play speed targeted for speed change if any. It is also the value displayed while
    /// the play speed is changing.
    targetspeed: Option<f64>,
    /// The current BPM. Can be negative, in that case the chart will scroll backwards.
    bpm: BPM,
    /// The timestamp at the last tick. It is a return value from `sdl::get_ticks` and measured
    /// in milliseconds.
    now: uint,
    /// The timestamp at the first tick.
    origintime: uint,

    /// A pointer to the point where the game play starts (and not necessarily equal to zero point
    /// in the chart). Corresponds to `origintime` and `TimelineInfo::originoffset`.
    origin: BmsPointer,
    /// A pointer to the grading line.
    cur: BmsPointer,
    /// A pointer to the lower bound of the grading area containing `cur`.
    checked: BmsPointer,
    /// A pointer to objects for the start of LN which grading is in progress.
    thru: ~[Option<BmsPointer>],
    /// The pointer to the first encountered `SetBPM` object with a negative value. This is
    /// a special casing for negative BPMs (ugh!); when this is set, the normal timeline routine is
    /// disabled and `cur` etc. always move backwards with given BPM. Everything except for
    /// `MeasureLine` is not rendered.
    reverse: Option<BmsPointer>,

    /// The scale factor for grading area. The factor less than 1 causes the grading area
    /// shrink.
    gradefactor: f64,
    /// The last grade and time when the grade is issued.
    lastgrade: Option<(Grade,uint)>,
    /// The numbers of each grades.
    gradecounts: [uint, ..NGRADES],
    /// The last combo number, i.e. the number of objects graded at least GREAT. GOOD doesn't
    /// cause the combo number reset; BAD and MISS do.
    lastcombo: uint,
    /// The best combo number so far. If the player manages to get no BADs and MISSes, then
    /// the combo number should end up with the number of note and LN objects
    /// (`BMSInfo::nnotes`).
    bestcombo: uint,
    /// The current score.
    score: uint,
    /// The current health gauge. Should be no larger than `MAXGAUGE`. This can go negative
    /// (not displayed directly), which will require players much more efforts to survive.
    gauge: int,
    /// The health gauge required to survive at the end of the song. Note that the gaugex
    /// less than this value (or even zero) doesn't cause the instant game over;
    /// only `InstantDeath` value from `Damage` does.
    survival: int,

    /// The number of keyboard or joystick keys, mapped to each lane and and currently pressed.
    keymultiplicity: [uint, ..NLANES],
    /// The state of joystick axes.
    joystate: [InputState, ..NLANES],
}

/// A list of play speed marks. `SpeedUpInput` and `SpeedDownInput` changes the play speed to
/// the next/previous nearest mark.
static SPEED_MARKS: &'static [f64] = &[0.1, 0.2, 0.4, 0.6, 0.8, 1.0, 1.2, 1.5, 2.0, 2.5, 3.0,
    3.5, 4.0, 4.5, 5.0, 5.5, 6.0, 7.0, 8.0, 10.0, 15.0, 25.0, 40.0, 60.0, 99.0];

/// Finds the next nearest play speed mark if any.
fn next_speed_mark(current: f64) -> Option<f64> {
    let mut prev = None;
    for &speed in SPEED_MARKS.iter() {
        if speed < current - 0.001 {
            prev = Some(speed);
        } else {
            return prev;
        }
    }
    None
}

/// Finds the previous nearest play speed mark if any.
fn previous_speed_mark(current: f64) -> Option<f64> {
    let mut next = None;
    for &speed in SPEED_MARKS.rev_iter() {
        if speed > current + 0.001 {
            next = Some(speed);
        } else {
            return next;
        }
    }
    None
}

/// Creates a beep sound played on the play speed change.
fn create_beep() -> ~sdl_mixer::Chunk {
    let samples = vec::from_fn::<i32>(12000, // approx. 0.14 seconds
        // sawtooth wave at 3150 Hz, quadratic decay after 0.02 seconds.
        |i| { let i = i as i32; (i%28-14) * cmp::min(2000, (12000-i)*(12000-i)/50000) });
    sdl_mixer::Chunk::new(unsafe { ::std::cast::transmute(samples) }, 128)
}

impl Player {
    /// Creates a new player object. The player object owns other related structures, including
    /// the options, BMS file, key specification, input mapping and sound resources.
    pub fn new(opts: Rc<Options>, bms: Bms, infos: TimelineInfo, keyspec: ~KeySpec,
               keymap: ~KeyMap, sndres: ~[SoundResource]) -> Player {
        // we no longer need the full `Bms` structure.
        let Bms { meta, timeline, .. } = bms;
        let timeline = Rc::new(timeline);

        let now = get_ticks();
        let initplayspeed = opts.borrow().playspeed;
        let originoffset = infos.originoffset;
        let gradefactor = 1.5 - cmp::min(meta.rank, 5) as f64 * 0.25;
        let initialgauge = MAXGAUGE * 500 / 1000;
        let survival = MAXGAUGE * 293 / 1000;
        let initbpm = timeline.borrow().initbpm;
        let nobjs = timeline.borrow().objs.len();
        let nsounds = sndres.len();

        // set all pointers to the origin and let the `tick` do the initial calculation
        let origin = timeline.pointer(VirtualPos, originoffset);
        let duration = timeline.borrow().duration(originoffset,
                                                  |sref| sndres[sref.to_uint()].duration());
        let mut player = Player {
            opts: opts, meta: meta, timeline: timeline, infos: infos, duration: duration,
            keyspec: keyspec, keymap: keymap,

            nograding: vec::from_elem(nobjs, false), sndres: sndres, beep: create_beep(),
            sndlastch: vec::from_elem(nsounds, None), lastchsnd: ~[], bga: initial_bga_state(),

            playspeed: initplayspeed, targetspeed: None, bpm: initbpm, now: now, origintime: now,

            origin: origin.clone(), cur: origin.clone(), checked: origin.clone(),
            thru: vec::from_fn(NLANES, |_| None), reverse: None,

            gradefactor: gradefactor, lastgrade: None, gradecounts: [0, ..NGRADES],
            lastcombo: 0, bestcombo: 0, score: 0, gauge: initialgauge, survival: survival,

            keymultiplicity: [0, ..NLANES], joystate: [Neutral, ..NLANES],
        };

        player.allocate_more_channels(64);
        sdl_mixer::reserve_channels(1); // so that the beep won't be affected
        player
    }

    /// Returns true if the specified lane is being pressed, either by keyboard, joystick
    /// buttons or axes.
    pub fn key_pressed(&self, lane: Lane) -> bool {
        let Lane(lane) = lane;
        self.keymultiplicity[lane] > 0 || self.joystate[lane] != Neutral
    }

    /// Returns the play speed displayed. Can differ from the actual play speed
    /// (`self.playspeed`) when the play speed is changing.
    pub fn nominal_playspeed(&self) -> f64 {
        self.targetspeed.unwrap_or(self.playspeed)
    }

    /// Updates the score and associated statistics according to grading. `scoredelta` is
    /// an weight normalized to [0,1] that is calculated from the distance between the object
    /// and the input time, and `damage` is an optionally associated `Damage` value for bombs.
    /// May return true when `Damage` resulted in the instant death.
    pub fn update_grade(&mut self, grade: Grade, scoredelta: f64,
                        damage: Option<Damage>) -> bool {
        self.gradecounts[grade as uint] += 1;
        self.lastgrade = Some((grade, self.now));
        self.score += (scoredelta * SCOREPERNOTE *
                       (1.0 + (self.lastcombo as f64) /
                              (self.infos.nnotes as f64))) as uint;

        match grade {
            MISS | BAD => { self.lastcombo = 0; }
            GOOD => {}
            GREAT | COOL => {
                // at most 5/512(1%) recover when the combo is topped
                let weight = if grade == GREAT {2} else {3};
                let cmbbonus = cmp::min(self.lastcombo as int, 100) / 50;
                self.lastcombo += 1;
                self.gauge = cmp::min(self.gauge + weight + cmbbonus, MAXGAUGE);
            }
        }
        self.bestcombo = cmp::max(self.bestcombo, self.lastcombo);

        match damage {
            Some(GaugeDamage(ratio)) => {
                self.gauge -= (MAXGAUGE as f64 * ratio) as int; true
            }
            Some(InstantDeath) => {
                self.gauge = cmp::min(self.gauge, 0); false
            }
            None => true
        }
    }

    /// Same as `update_grade`, but the grade is calculated from the normalized difference
    /// between the object and input time in seconds. The normalized distance equals to
    /// the actual time difference when `gradefactor` is 1.0.
    pub fn update_grade_from_distance(&mut self, dist: f64) {
        let dist = num::abs(dist);
        let (grade, damage) = if      dist <  COOL_CUTOFF {(COOL,None)}
                              else if dist < GREAT_CUTOFF {(GREAT,None)}
                              else if dist <  GOOD_CUTOFF {(GOOD,None)}
                              else if dist <   BAD_CUTOFF {(BAD,Some(BAD_DAMAGE))}
                              else                        {(MISS,Some(MISS_DAMAGE))};
        let scoredelta = cmp::max(1.0 - dist / BAD_CUTOFF, 0.0);
        let keepgoing = self.update_grade(grade, scoredelta, damage);
        assert!(keepgoing);
    }

    /// Same as `update_grade`, but with the predetermined damage value. Always results in MISS
    /// grade. May return true when the damage resulted in the instant death.
    pub fn update_grade_from_damage(&mut self, damage: Damage) -> bool {
        self.update_grade(MISS, 0.0, Some(damage))
    }

    /// Same as `update_grade`, but always results in MISS grade with the standard damage value.
    pub fn update_grade_to_miss(&mut self) {
        let keepgoing = self.update_grade(MISS, 0.0, Some(MISS_DAMAGE));
        assert!(keepgoing);
    }

    /// Allocate more SDL_mixer channels without stopping already playing channels.
    pub fn allocate_more_channels(&mut self, howmany: uint) {
        let howmany = howmany as ::std::libc::c_int;
        let nchannels = sdl_mixer::allocate_channels(-1 as ::std::libc::c_int);
        let nchannels = sdl_mixer::allocate_channels(nchannels + howmany) as uint;
        if self.lastchsnd.len() < nchannels {
            self.lastchsnd.grow(nchannels, &None);
        }
    }

    /// Plays a given sound referenced by `sref`. `bgm` indicates that the sound is a BGM and
    /// should be played with the lower volume and should in the different channel group from
    /// key sounds.
    pub fn play_sound(&mut self, sref: SoundRef, bgm: bool) {
        let sref = sref.to_uint();
        if self.sndres[sref].chunk().is_none() {
            return;
        }
        let lastch = self.sndlastch[sref].map(|ch| ch as ::std::libc::c_int);

        // try to play on the last channel if it is not occupied by other sounds (in this case
        // the last channel info is removed)
        let mut ch;
        loop {
            ch = self.sndres[sref].chunk().unwrap().play(lastch, 0);
            if ch >= 0 { break; }
            self.allocate_more_channels(32);
        }

        let group = if bgm {1} else {0};
        sdl_mixer::set_channel_volume(Some(ch), if bgm {96} else {128});
        sdl_mixer::group_channel(Some(ch), Some(group));

        let ch = ch as uint;
        for &idx in self.lastchsnd[ch].iter() { self.sndlastch[idx] = None; }
        self.sndlastch[sref] = Some(ch);
        self.lastchsnd[ch] = Some(sref as uint);
    }

    /// Plays a given sound if `sref` is not zero. This reflects the fact that an alphanumeric
    /// key `00` is normally a placeholder.
    pub fn play_sound_if_nonzero(&mut self, sref: SoundRef, bgm: bool) {
        if sref.to_uint() > 0 { self.play_sound(sref, bgm); }
    }

    /// Plays a beep. The beep is always played in the channel 0, which is excluded from
    /// the uniform key sound and BGM management.
    pub fn play_beep(&self) {
        self.beep.play(Some(0), 0);
    }

    /// Updates the player state. Returns `true` if the caller should keep calling `tick`.
    pub fn tick(&mut self) -> bool {
        let opts_ = self.opts.clone();
        let opts = opts_.borrow();

        // smoothly change the play speed
        if self.targetspeed.is_some() {
            let target = self.targetspeed.unwrap();
            let delta = target - self.playspeed;
            if num::abs(delta) < 0.001 {
                self.playspeed = target;
                self.targetspeed = None;
            } else {
                self.playspeed += delta * 0.1;
            }
        }

        self.now = get_ticks();
        let mut cur = self.cur.clone();
        let mut checked = self.checked.clone();
        let mut thru = self.thru.clone();
        let prev = self.cur.clone();

        let curtime = (self.now - self.origintime) as f64 / 1000.0 + self.origin.loc.time;
        match self.reverse {
            Some(ref reverse) => {
                assert!(self.bpm.to_f64() < 0.0 && curtime >= reverse.loc.time);
                let newpos = reverse.loc.pos + self.bpm.sec_to_measure(curtime - reverse.loc.time);
                cur.seek(ActualPos, newpos - cur.loc.pos);
            }
            None => {
                // apply object-like effects while advancing `self.cur`
                for p in cur.mut_until(ActualTime, curtime - cur.loc.time) {
                    match p.data() {
                        BGM(sref) => {
                            self.play_sound_if_nonzero(sref, true);
                        }
                        SetBGA(layer, bgaref) => {
                            self.bga[layer as uint] = bgaref;
                        }
                        SetBPM(newbpm) => {
                            self.bpm = newbpm;
                            if newbpm.to_f64() == 0.0 {
                                return false; // finish immediately
                            } else if newbpm.to_f64() < 0.0 {
                                self.reverse = Some(p.clone()); // activate reverse motion
                            }
                        }
                        Visible(_,sref) | LNStart(_,sref) => {
                            if opts.is_autoplay() {
                                for &sref in sref.iter() {
                                    self.play_sound_if_nonzero(sref, false);
                                }
                                self.update_grade_from_distance(0.0);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // grade objects that have escaped the grading area
        if !opts.is_autoplay() {
            for p in checked.mut_upto(&cur) {
                let dist = (cur.loc.vtime - p.loc.vtime) * self.gradefactor;
                if dist < BAD_CUTOFF { break; }

                if !self.nograding[p.index] {
                    let lane = p.object_lane(); // XXX #3511
                    for &Lane(lane) in lane.iter() {
                        let missable =
                            match p.data() {
                                Visible(..) | LNStart(..) => true,
                                LNDone(..) => thru[lane].is_some(),
                                _ => false,
                            };
                        if missable {
                            self.update_grade_to_miss();
                            thru[lane] = None;
                        }
                    }
                }
            }
        }

        // process inputs
        loop {
            // map to the virtual input. results in `vkey` (virtual key), `state` (input state)
            // and `continuous` (true if the input is not discrete and `Negative` input state
            // matters).
            let (key, state) = match event::poll_event() {
                event::NoEvent => { break; }
                ev => match Input::from_event(ev) {
                    Some((QuitInput,_)) => { return false; },
                    Some(key_and_state) => key_and_state,
                    None => { continue; }
                }
            };
            let vkey = match self.keymap.find(&key) {
                Some(&vkey) => vkey,
                None => { continue; }
            };
            let continuous = match key {
                KeyInput(..) | JoyButtonInput(..) | QuitInput => false,
                JoyAxisInput(..) => true
            };

            if opts.is_exclusive() { continue; }

            // Returns true if the given lane is previously pressed and now unpressed.
            // When the virtual input is mapped to multiple actual inputs it can update
            // the internal state but still return false.
            let is_unpressed = |lane: Lane, continuous: bool, state: InputState| {
                let lane = lane.to_uint();
                if state == Neutral || (continuous && self.joystate[lane] != state) {
                    if continuous {
                        self.joystate[lane] = state; true
                    } else {
                        if self.keymultiplicity[lane] > 0 {
                            self.keymultiplicity[lane] -= 1;
                        }
                        (self.keymultiplicity[lane] == 0)
                    }
                } else {
                    false
                }
            };

            // Returns true if the given lane is previously unpressed and now pressed.
            // When the virtual input is mapped to multiple actual inputs it can update
            // the internal state but still return false.
            let is_pressed = |lane: Lane, continuous: bool, state: InputState| {
                let lane = lane.to_uint();
                if state != Neutral {
                    if continuous {
                        self.joystate[lane] = state; true
                    } else {
                        self.keymultiplicity[lane] += 1;
                        (self.keymultiplicity[lane] == 1)
                    }
                } else {
                    false
                }
            };

            let process_unpress = |lane: Lane| {
                // if LN grading is in progress and it is not within the threshold then
                // MISS grade is issued
                for thru in thru[lane.to_uint()].iter() {
                    let nextlndone = thru.find_next_of_type(|obj| {
                        obj.object_lane() == Some(lane) && obj.is_lndone()
                    });
                    for p in nextlndone.iter() {
                        let delta = (p.loc.vtime - cur.loc.vtime) * self.gradefactor;
                        if num::abs(delta) < BAD_CUTOFF {
                            self.nograding[p.index] = true;
                        } else {
                            self.update_grade_to_miss();
                        }
                    }
                }
                thru[lane.to_uint()] = None;
            };

            let process_press = |lane: Lane| {
                // plays the closest key sound
                let soundable =
                    cur.find_closest_of_type(VirtualTime, |obj| {
                        obj.object_lane() == Some(lane) && obj.is_soundable()
                    });
                for p in soundable.iter() {
                    let sounds = p.sounds(); // XXX #3511
                    for &sref in sounds.iter() {
                        self.play_sound(sref, false);
                    }
                }

                // tries to grade the closest gradable object in the grading area
                let gradable =
                    cur.find_closest_of_type(VirtualTime, |obj| {
                        obj.object_lane() == Some(lane) && obj.is_gradable()
                    });
                for p in gradable.iter() {
                    if p.index >= checked.index && !self.nograding[p.index] && !p.is_lndone() {
                        let dist = (p.loc.vtime - cur.loc.vtime) * self.gradefactor;
                        if num::abs(dist) < BAD_CUTOFF {
                            if p.is_lnstart() { thru[lane.to_uint()] = Some(p.clone()); }
                            self.nograding[p.index] = true;
                            self.update_grade_from_distance(dist);
                        }
                    }
                }
                true
            };

            match (vkey, state) {
                (SpeedDownInput, Positive) | (SpeedDownInput, Negative) => {
                    let current = self.targetspeed.unwrap_or(self.playspeed);
                    let newspeed = next_speed_mark(current); // XXX #3511
                    for &newspeed in newspeed.iter() {
                        self.targetspeed = Some(newspeed);
                        self.play_beep();
                    }
                }
                (SpeedUpInput, Positive) | (SpeedUpInput, Negative) => {
                    let current = self.targetspeed.unwrap_or(self.playspeed);
                    let newspeed = previous_speed_mark(current); // XXX #3511
                    for &newspeed in newspeed.iter() {
                        self.targetspeed = Some(newspeed);
                        self.play_beep();
                    }
                }
                (LaneInput(lane), state) => {
                    if !opts.is_autoplay() {
                        if is_unpressed(lane, continuous, state) {
                            process_unpress(lane);
                        }
                        if is_pressed(lane, continuous, state) {
                            process_press(lane);
                        }
                    }
                }
                (_, _) => {}
            }

        }

        // process bombs
        if !opts.is_autoplay() {
            for p in prev.upto(&cur) {
                match p.data() {
                    Bomb(lane,sref,damage) if self.key_pressed(lane) => {
                        // ongoing long note is not graded twice
                        thru[lane.to_uint()] = None;
                        for &sref in sref.iter() {
                            self.play_sound(sref, false);
                        }
                        if !self.update_grade_from_damage(damage) {
                            // instant death
                            self.cur = cur.find_end();
                            return false;
                        }
                    }
                    _ => {}
                }
            }
        }

        self.cur = cur;
        self.checked = checked;
        self.thru = thru;

        // determines if we should keep playing
        if self.cur.index == self.timeline.borrow().objs.len() {
            if opts.is_autoplay() {
                sdl_mixer::num_playing(None) != sdl_mixer::num_playing(Some(0))
            } else {
                sdl_mixer::newest_in_group(Some(1)).is_some()
            }
        } else if self.cur.loc.vpos < self.infos.originoffset {
            false // special casing the negative BPM
        } else {
            true
        }
    }
}

#[unsafe_destructor]
impl Drop for Player {
    fn drop(&mut self) {
        // remove all channels before sound resources are deallocated.
        // halting alone is not sufficient due to rust-sdl's bug.
        sdl_mixer::allocate_channels(0);
    }
}

