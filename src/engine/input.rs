// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, 2014, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! Mapping from actual inputs to virtual inputs.

use std::hash;
use std::collections::HashMap;
use sdl::event;
use sdl::event::Event;

use format::obj::Lane;
use format::bms::Key;
use engine::keyspec::{KeySpec, KeyKind};

/// Actual input. Mapped to zero or more virtual inputs by input mapping.
#[deriving(PartialEq,Eq)]
pub enum Input {
    /// Keyboard input.
    Key(event::Key),
    /// Joystick axis input.
    JoyAxis(uint),
    /// Joystick button input.
    JoyButton(uint),
    /// A special input generated by pressing the quit button or escape key.
    Quit,
}

impl hash::Hash for Input {
    fn hash(&self, state: &mut hash::sip::SipState) {
        match *self {
            Input::Key(key) => { 0u8.hash(state); (key as uint).hash(state); }
            Input::JoyAxis(axis) => { 1u8.hash(state); axis.hash(state); }
            Input::JoyButton(button) => { 2u8.hash(state); button.hash(state); }
            Input::Quit => { 3u8.hash(state); }
        }
    }
}

impl Input {
    /// Translates an SDL event to the (internal) actual input type and state.
    pub fn from_event(event: Event) -> Option<(Input, InputState)> {
        match event {
            Event::Quit | Event::Key(event::Key::Escape,_,_,_) =>
                Some((Input::Quit, InputState::Positive)),
            Event::Key(key,true,_,_) =>
                Some((Input::Key(key), InputState::Positive)),
            Event::Key(key,false,_,_) =>
                Some((Input::Key(key), InputState::Neutral)),
            Event::JoyButton(_which,button,true) =>
                Some((Input::JoyButton(button as uint), InputState::Positive)),
            Event::JoyButton(_which,button,false) =>
                Some((Input::JoyButton(button as uint), InputState::Neutral)),
            Event::JoyAxis(_which,axis,delta) if delta > 3200 =>
                Some((Input::JoyAxis(axis as uint), InputState::Positive)),
            Event::JoyAxis(_which,axis,delta) if delta < -3200 =>
                Some((Input::JoyAxis(axis as uint), InputState::Negative)),
            Event::JoyAxis(_which,axis,_delta) =>
                Some((Input::JoyAxis(axis as uint), InputState::Neutral)),
            _ => None
        }
    }
}

/// Virtual input.
#[deriving(PartialEq,Eq)]
pub enum VirtualInput {
    /// Virtual input mapped to the lane.
    Lane(Lane),
    /// Speed down input (normally F3).
    SpeedDown,
    /// Speed up input (normally F4).
    SpeedUp,
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
#[deriving(PartialEq,Eq)]
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
            VirtualInput::Lane(Lane(lane)) => keyspec.kinds[lane] == Some(kind),
            VirtualInput::SpeedDown | VirtualInput::SpeedUp => true
        }
    }
}

/// An information about an environment variable for multiple keys.
struct KeySet {
    envvar: &'static str,
    envvar2: &'static str, // for compatibility with Angolmois
    default: &'static str,
    mapping: &'static [(Option<KeyKind>, &'static [VirtualInput])],
}

/// A list of environment variables that set the mapping for multiple keys, and corresponding
/// default values and the order of keys.
static KEYSETS: &'static [KeySet] = &[
    KeySet { envvar: "SNRS_1P_KEYS",
             envvar2: "ANGOLMOIS_1P_KEYS",
             default: "left shift%axis 3|z%button 3|s%button 6|x%button 2|d%button 7|\
                       c%button 1|f%button 4|v%axis 2|left alt",
             mapping: &[(Some(KeyKind::Scratch),   &[VirtualInput::Lane(Lane(6))]),
                        (Some(KeyKind::WhiteKey),  &[VirtualInput::Lane(Lane(1))]),
                        (Some(KeyKind::BlackKey),  &[VirtualInput::Lane(Lane(2))]),
                        (Some(KeyKind::WhiteKey),  &[VirtualInput::Lane(Lane(3))]),
                        (Some(KeyKind::BlackKey),  &[VirtualInput::Lane(Lane(4))]),
                        (Some(KeyKind::WhiteKey),  &[VirtualInput::Lane(Lane(5))]),
                        (Some(KeyKind::BlackKey),  &[VirtualInput::Lane(Lane(8))]),
                        (Some(KeyKind::WhiteKey),  &[VirtualInput::Lane(Lane(9))]),
                        (Some(KeyKind::FootPedal), &[VirtualInput::Lane(Lane(7))])] },
    KeySet { envvar: "SNRS_2P_KEYS",
             envvar2: "ANGOLMOIS_2P_KEYS",
             default: "right alt|m|k|,|l|.|;|/|right shift",
             mapping: &[(Some(KeyKind::FootPedal), &[VirtualInput::Lane(Lane(36+7))]),
                        (Some(KeyKind::WhiteKey),  &[VirtualInput::Lane(Lane(36+1))]),
                        (Some(KeyKind::BlackKey),  &[VirtualInput::Lane(Lane(36+2))]),
                        (Some(KeyKind::WhiteKey),  &[VirtualInput::Lane(Lane(36+3))]),
                        (Some(KeyKind::BlackKey),  &[VirtualInput::Lane(Lane(36+4))]),
                        (Some(KeyKind::WhiteKey),  &[VirtualInput::Lane(Lane(36+5))]),
                        (Some(KeyKind::BlackKey),  &[VirtualInput::Lane(Lane(36+8))]),
                        (Some(KeyKind::WhiteKey),  &[VirtualInput::Lane(Lane(36+9))]),
                        (Some(KeyKind::Scratch),   &[VirtualInput::Lane(Lane(36+6))])] },
    KeySet { envvar: "SNRS_PMS_KEYS",
             envvar2: "ANGOLMOIS_PMS_KEYS",
             default: "z|s|x|d|c|f|v|g|b",
             mapping: &[(Some(KeyKind::Button1), &[VirtualInput::Lane(Lane(1))]),
                        (Some(KeyKind::Button2), &[VirtualInput::Lane(Lane(2))]),
                        (Some(KeyKind::Button3), &[VirtualInput::Lane(Lane(3))]),
                        (Some(KeyKind::Button4), &[VirtualInput::Lane(Lane(4))]),
                        (Some(KeyKind::Button5), &[VirtualInput::Lane(Lane(5))]),
                        (Some(KeyKind::Button4), &[VirtualInput::Lane(Lane(8)),
                                                   VirtualInput::Lane(Lane(36+2))]),
                        (Some(KeyKind::Button3), &[VirtualInput::Lane(Lane(9)),
                                                   VirtualInput::Lane(Lane(36+3))]),
                        (Some(KeyKind::Button2), &[VirtualInput::Lane(Lane(6)),
                                                   VirtualInput::Lane(Lane(36+4))]),
                        (Some(KeyKind::Button1), &[VirtualInput::Lane(Lane(7)),
                                                   VirtualInput::Lane(Lane(36+5))])] },
    KeySet { envvar: "SNRS_SPEED_KEYS",
             envvar2: "ANGOLMOIS_SPEED_KEYS",
             default: "f3|f4",
             mapping: &[(None, &[VirtualInput::SpeedDown]),
                        (None, &[VirtualInput::SpeedUp])] },
];

/// An input mapping, i.e. a mapping from the actual input to the virtual input.
pub type KeyMap = HashMap<Input,VirtualInput>;

/// Reads an input mapping from the environment variables.
pub fn read_keymap(keyspec: &KeySpec, getenv: |&str| -> Option<String>) -> Result<KeyMap,String> {
    use std::ascii::{AsciiExt, OwnedAsciiExt};

    /// Finds an SDL virtual key with the given name. Matching is done case-insensitively.
    fn sdl_key_from_name(name: &str) -> Option<event::Key> {
        let name = name.to_ascii_lower();
        unsafe {
            let firstkey = 0u16;
            let lastkey = ::std::mem::transmute(event::Key::Last);
            for keyidx in range(firstkey, lastkey) {
                let key = ::std::mem::transmute(keyidx);
                let keyname = event::get_key_name(key).into_ascii_lower();
                if keyname == name { return Some(key); }
            }
        }
        None
    }

    /// Parses an `Input` value from the string. E.g. `"backspace"`, `"button 2"` or `"axis 0"`.
    fn parse_input(s: &str) -> Option<Input> {
        let mut idx = 0;
        let s = s.trim();
        if lex!(s; lit "button", ws, uint -> idx) {
            Some(Input::JoyButton(idx))
        } else if lex!(s; lit "axis", ws, uint -> idx) {
            Some(Input::JoyAxis(idx))
        } else {
            sdl_key_from_name(s).map(|key| Input::Key(key))
        }
    }

    let mut map = HashMap::new();
    let add_mapping = |map: &mut KeyMap, kind: Option<KeyKind>,
                       input: Input, vinput: VirtualInput| {
        if kind.map_or(true, |kind| vinput.active_in_key_spec(kind, keyspec)) {
            map.insert(input, vinput);
        }
    };

    for &keyset in KEYSETS.iter() {
        let spec = getenv(keyset.envvar).or(getenv(keyset.envvar2));
        let spec = spec.unwrap_or(keyset.default.to_string());

        let mut i = 0;
        for part in spec[].split('|') {
            let (kind, vinputs) = keyset.mapping[i];
            for s in part.split('%') {
                match parse_input(s[]) {
                    Some(input) => {
                        for &vinput in vinputs.iter() {
                            add_mapping(&mut map, kind, input, vinput);
                        }
                    }
                    None => {
                        return Err(format!("Unknown key name in the environment variable {}: {}",
                                           keyset.envvar, s));
                    }
                }
            }

            i += 1;
            if i >= keyset.mapping.len() { break; }
        }
    }

    for &lane in keyspec.order.iter() {
        let key = Key(36 + *lane as int);
        let kind = keyspec.kinds[*lane].unwrap();
        let envvar = format!("SNRS_{}{}_KEY", key, kind.to_char());
        let envvar2 = format!("ANGOLMOIS_{}{}_KEY", key, kind.to_char());
        for s in getenv(envvar[]).or(getenv(envvar2[])).iter() {
            match parse_input(s[]) {
                Some(input) => {
                    add_mapping(&mut map, Some(kind), input, VirtualInput::Lane(lane));
                }
                None => {
                    return Err(format!("Unknown key name in the environment variable {}: {}",
                                       envvar, *s));
                }
            }
        }
    }

    Ok(map)
}

