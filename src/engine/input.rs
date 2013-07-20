// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! Mapping from actual inputs to virtual inputs.

use std::{uint, to_bytes};

use sdl::event;
use format::obj::Lane;
use format::bms::Key;
use engine::keyspec::*;

/// Actual input. Mapped to zero or more virtual inputs by input mapping.
#[deriving(Eq)]
pub enum Input {
    /// Keyboard input.
    KeyInput(event::Key),
    /// Joystick axis input.
    JoyAxisInput(uint),
    /// Joystick button input.
    JoyButtonInput(uint),
    /// A special input generated by pressing the quit button or escape key.
    QuitInput,
}

impl IterBytes for Input {
    fn iter_bytes(&self, lsb0: bool, f: to_bytes::Cb) -> bool {
        match *self {
            KeyInput(key) => // XXX #7363
                0u8.iter_bytes(lsb0, |b| f(b)) && (key as uint).iter_bytes(lsb0, |b| f(b)),
            JoyAxisInput(axis) => // XXX #7363
                1u8.iter_bytes(lsb0, |b| f(b)) && axis.iter_bytes(lsb0, |b| f(b)),
            JoyButtonInput(button) => // XXX #7363
                2u8.iter_bytes(lsb0, |b| f(b)) && button.iter_bytes(lsb0, |b| f(b)),
            QuitInput => // XXX #7363
                3u8.iter_bytes(lsb0, |b| f(b))
        }
    }
}

impl Input {
    /// Translates an SDL event to the (internal) actual input type and state.
    pub fn from_event(event: event::Event) -> Option<(Input, InputState)> {
        match event {
            event::QuitEvent | event::KeyEvent(event::EscapeKey,_,_,_) =>
                Some((QuitInput, Positive)),
            event::KeyEvent(key,true,_,_) =>
                Some((KeyInput(key), Positive)),
            event::KeyEvent(key,false,_,_) =>
                Some((KeyInput(key), Neutral)),
            event::JoyButtonEvent(_which,button,true) =>
                Some((JoyButtonInput(button as uint), Positive)),
            event::JoyButtonEvent(_which,button,false) =>
                Some((JoyButtonInput(button as uint), Neutral)),
            event::JoyAxisEvent(_which,axis,delta) if delta > 3200 =>
                Some((JoyAxisInput(axis as uint), Positive)),
            event::JoyAxisEvent(_which,axis,delta) if delta < -3200 =>
                Some((JoyAxisInput(axis as uint), Negative)),
            event::JoyAxisEvent(_which,axis,_delta) =>
                Some((JoyAxisInput(axis as uint), Neutral)),
            _ => None
        }
    }
}

/// Virtual input.
#[deriving(Eq)]
pub enum VirtualInput {
    /// Virtual input mapped to the lane.
    LaneInput(Lane),
    /// Speed down input (normally F3).
    SpeedDownInput,
    /// Speed up input (normally F4).
    SpeedUpInput,
}

/**
 * State of virtual input elements. There are three states: neutral, and positive or negative.
 * There is no difference between positive and negative states (the naming is arbitrary)
 * except for that they are distinct.
 *
 * The states should really be one of pressed (non-neutral) or unpressed (neutral) states,
 * but we need two non-neutral states since the actual input device with continuous values
 * (e.g. joystick axes) can trigger the state transition *twice* without hitting the neutral
 * state. We solve this problem by making the transition from negative to positive (and vice
 * versa) temporarily hit the neutral state.
 */
#[deriving(Eq)]
pub enum InputState {
    /// Positive input state. Occurs when the button is pressed or the joystick axis is moved
    /// in the positive direction.
    Positive = 1,
    /// Neutral input state. Occurs when the button is not pressed or the joystick axis is moved
    /// back to the origin.
    Neutral = 0,
    /// Negative input state. Occurs when the joystick axis is moved in the negative direction.
    Negative = -1
}

impl VirtualInput {
    /// Returns true if the virtual input has a specified key kind in the key specification.
    pub fn active_in_key_spec(&self, kind: KeyKind, keyspec: &KeySpec) -> bool {
        match *self {
            LaneInput(Lane(lane)) => keyspec.kinds[lane] == Some(kind),
            SpeedDownInput | SpeedUpInput => true
        }
    }
}

/// An information about an environment variable for multiple keys.
//
// Rust: static struct seems not working somehow... (#5688)
/*
struct KeySet {
    envvar: &'static str,
    envvar2: &'static str, // for compatibility with Angolmois
    default: &'static str,
    mapping: &'static [(Option<KeyKind>, &'static [VirtualInput])],
}
*/
type KeySet = (
    &'static str,
    &'static str,
    &'static str,
    &'static [(Option<KeyKind>, &'static [VirtualInput])]);

/// A list of environment variables that set the mapping for multiple keys, and corresponding
/// default values and the order of keys.
static KEYSETS: &'static [KeySet] = &[
    (/*KeySet { envvar:*/ &"SNRS_1P_KEYS",
             /*envvar2:*/ &"ANGOLMOIS_1P_KEYS",
             /*default:*/ &"left shift%axis 3|z%button 3|s%button 6|x%button 2|d%button 7|\
                        c%button 1|f%button 4|v%axis 2|left alt",
             /*mapping:*/ &[(Some(Scratch),   &[LaneInput(Lane(6))]),
                        (Some(WhiteKey),  &[LaneInput(Lane(1))]),
                        (Some(BlackKey),  &[LaneInput(Lane(2))]),
                        (Some(WhiteKey),  &[LaneInput(Lane(3))]),
                        (Some(BlackKey),  &[LaneInput(Lane(4))]),
                        (Some(WhiteKey),  &[LaneInput(Lane(5))]),
                        (Some(BlackKey),  &[LaneInput(Lane(8))]),
                        (Some(WhiteKey),  &[LaneInput(Lane(9))]),
                        (Some(FootPedal), &[LaneInput(Lane(7))])] /*}*/),
    (/*KeySet { envvar:*/ &"SNRS_2P_KEYS",
             /*envvar2:*/ &"ANGOLMOIS_2P_KEYS",
             /*default:*/ &"right alt|m|k|,|l|.|;|/|right shift",
             /*mapping:*/ &[(Some(FootPedal), &[LaneInput(Lane(36+7))]),
                        (Some(WhiteKey),  &[LaneInput(Lane(36+1))]),
                        (Some(BlackKey),  &[LaneInput(Lane(36+2))]),
                        (Some(WhiteKey),  &[LaneInput(Lane(36+3))]),
                        (Some(BlackKey),  &[LaneInput(Lane(36+4))]),
                        (Some(WhiteKey),  &[LaneInput(Lane(36+5))]),
                        (Some(BlackKey),  &[LaneInput(Lane(36+8))]),
                        (Some(WhiteKey),  &[LaneInput(Lane(36+9))]),
                        (Some(Scratch),   &[LaneInput(Lane(36+6))])] ),
    (/*KeySet { envvar:*/ &"SNRS_PMS_KEYS",
             /*envvar2:*/ &"ANGOLMOIS_PMS_KEYS",
             /*default:*/ &"z|s|x|d|c|f|v|g|b",
             /*mapping:*/ &[(Some(Button1), &[LaneInput(Lane(1))]),
                        (Some(Button2), &[LaneInput(Lane(2))]),
                        (Some(Button3), &[LaneInput(Lane(3))]),
                        (Some(Button4), &[LaneInput(Lane(4))]),
                        (Some(Button5), &[LaneInput(Lane(5))]),
                        (Some(Button4), &[LaneInput(Lane(8)), LaneInput(Lane(36+2))]),
                        (Some(Button3), &[LaneInput(Lane(9)), LaneInput(Lane(36+3))]),
                        (Some(Button2), &[LaneInput(Lane(6)), LaneInput(Lane(36+4))]),
                        (Some(Button1), &[LaneInput(Lane(7)), LaneInput(Lane(36+5))])] ),
    (/*KeySet { envvar:*/ &"SNRS_SPEED_KEYS",
             /*envvar2:*/ &"ANGOLMOIS_SPEED_KEYS",
             /*default:*/ &"f3|f4",
             /*mapping:*/ &[(None, &[SpeedDownInput]),
                        (None, &[SpeedUpInput])] ),
];

/// An input mapping, i.e. a mapping from the actual input to the virtual input.
pub type KeyMap = ::std::hashmap::HashMap<Input,VirtualInput>;

/// Reads an input mapping from the environment variables.
pub fn read_keymap(keyspec: &KeySpec, getenv: &fn(&str) -> Option<~str>) -> Result<KeyMap,~str> {
    use util::std::str::StrUtil;

    /// Finds an SDL virtual key with the given name. Matching is done case-insensitively.
    fn sdl_key_from_name(name: &str) -> Option<event::Key> {
        let name = name.to_ascii_lower();
        unsafe {
            let firstkey = 0;
            let lastkey = ::std::cast::transmute(event::LastKey);
            for uint::range(firstkey, lastkey) |keyidx| {
                let key = ::std::cast::transmute(keyidx);
                let keyname = event::get_key_name(key).to_ascii_lower();
                if keyname == name { return Some(key); }
            }
        }
        None
    }

    /// Parses an `Input` value from the string. E.g. `"backspace"`, `"button 2"` or `"axis 0"`.
    fn parse_input(s: &str) -> Option<Input> {
        let mut idx = 0;
        let s = s.trim();
        if lex!(s; "button", ws, uint -> idx) {
            Some(JoyButtonInput(idx))
        } else if lex!(s; "axis", ws, uint -> idx) {
            Some(JoyAxisInput(idx))
        } else {
            sdl_key_from_name(s).map(|&key| KeyInput(key))
        }
    }

    let mut map = ::std::hashmap::HashMap::new();
    let add_mapping = |kind: Option<KeyKind>, input: Input, vinput: VirtualInput| {
        if kind.map_default(true, |&kind| vinput.active_in_key_spec(kind, keyspec)) {
            map.insert(input, vinput);
        }
    };

    for KEYSETS.iter().advance |&keyset| {
        let (envvar, envvar2, default, mapping) = keyset; // XXX
        let spec = getenv(/*keyset.*/envvar).or(getenv(/*keyset.*/envvar2));
        let spec = spec.get_or_default(/*keyset.*/default.to_owned());

        let mut i = 0;
        for spec.split_iter('|').advance |part| {
            let (kind, vinputs) = /*keyset.*/mapping[i];
            for part.split_iter('%').advance |s| {
                match parse_input(s) {
                    Some(input) => {
                        for vinputs.iter().advance |&vinput| {
                            add_mapping(kind, input, vinput);
                        }
                    }
                    None => {
                        return Err(fmt!("Unknown key name in the environment variable %s: %s",
                                        /*keyset.*/envvar, s));
                    }
                }
            }

            i += 1;
            if i >= /*keyset.*/mapping.len() { break; }
        }
    }

    for keyspec.order.iter().advance |&lane| {
        let key = Key(36 + *lane as int);
        let kind = keyspec.kinds[*lane].get();
        let envvar = fmt!("SNRS_%s%c_KEY", key.to_str(), kind.to_char());
        let envvar2 = fmt!("ANGOLMOIS_%s%c_KEY", key.to_str(), kind.to_char());
        let val = getenv(envvar).or(getenv(envvar2)); // XXX #3511
        for val.iter().advance |&s| {
            match parse_input(s) {
                Some(input) => { add_mapping(Some(kind), input, LaneInput(lane)); }
                None => { return Err(fmt!("Unknown key name in the environment variable %s: %s",
                                          envvar, s)); }
            }
        }
    }

    Ok(map)
}

