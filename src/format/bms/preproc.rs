// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! BMS preprocessor.

use std::rand::Rng;

use format::bms::diag::*;

/// Represents one line of BMS command that may affect the control flow.
#[deriving(Clone)]
pub enum BmsFlowCommand {
    BmsRandom(int),                             // #RANDOM
    BmsSetRandom(int),                          // #SETRANDOM
    BmsEndRandom,                               // #ENDRANDOM
    BmsIf(int),                                 // #IF
    BmsElseIf(int),                             // #ELSEIF
    BmsElse,                                    // #ELSE
    BmsEndIf,                                   // #ENDIF
    BmsSwitch(int),                             // #SWITCH
    BmsSetSwitch(int),                          // #SETSWITCH
    BmsEndSw,                                   // #ENDSW
    BmsCase(int),                               // #CASE
    BmsSkip,                                    // #SKIP
    BmsDef,                                     // #DEF
}

impl ToStr for BmsFlowCommand {
    /// Returns a reconstructed line for given BMS flow command.
    fn to_str(&self) -> ~str {
        match *self {
            BmsRandom(val) => format!("\\#RANDOM {}", val),
            BmsSetRandom(val) => format!("\\#SETRANDOM {}", val),
            BmsEndRandom => ~"#ENDRANDOM",
            BmsIf(val) => format!("\\#IF {}", val),
            BmsElseIf(val) => format!("\\#ELSEIF {}", val),
            BmsElse => ~"#ELSE",
            BmsEndIf => ~"#ENDIF",
            BmsSwitch(val) => format!("\\#SWITCH {}", val),
            BmsSetSwitch(val) => format!("\\#SETSWITCH {}", val),
            BmsEndSw => ~"#ENDSW",
            BmsCase(val) => format!("\\#CASE {}", val),
            BmsSkip => ~"#SKIP",
            BmsDef => ~"#DEF",
        }
    }
}

/// The state of the block, for determining which lines should be processed.
#[deriving(Eq)]
enum BlockState {
    /// Not contained in the #IF block.
    Outside,
    /// Active.
    Process,
    /// Inactive, but (for the purpose of #IF/#ELSEIF/#ELSE/#ENDIF structure) can move to
    /// `Process` state when matching clause appears.
    Ignore,
    /// Inactive and won't be processed until the end of block.
    NoFurther
}

impl BlockState {
    /// Returns true if lines should be ignored in the current block given that the parent
    /// block was active.
    fn inactive(self) -> bool {
        match self { Outside | Process => false, Ignore | NoFurther => true }
    }
}

/**
 * Block information. The parser keeps a list of nested blocks and determines if
 * a particular line should be processed or not.
 *
 * Sonorous actually recognizes only one kind of blocks, starting with #RANDOM or
 * #SETRANDOM and ending with #ENDRANDOM or #END(IF) outside an #IF block. An #IF block is
 * a state within #RANDOM, so it follows that #RANDOM/#SETRANDOM blocks can nest but #IF
 * can't nest unless its direct parent is #RANDOM/#SETRANDOM.
 */
#[deriving(Eq)]
struct Block {
    /// A generated value if any. It can be `None` if this block is the topmost one (which
    /// is actually not a block but rather a sentinel) or the last `#RANDOM` or `#SETRANDOM`
    /// command was invalid, and #IF in that case will always evaluates to false.
    val: Option<int>,
    /// The state of the block.
    state: BlockState,
    /// True if the parent block is already ignored so that this block should be ignored
    /// no matter what `state` is.
    skip: bool
}

/// A generic BMS preprocessor. `T` is normally a BMS command, but there is no restriction.
pub struct Preprocessor<'self,T,R,Listener> {
    /// The current block informations.
    blocks: ~[Block],
    /// Random number generator.
    r: &'self mut R,
    /// Message callback.
    callback: &'self mut Listener,
}

impl<'self,T:Send+Clone,R:Rng,Listener:BmsMessageListener> Preprocessor<'self,T,R,Listener> {
    /// Creates a new preprocessor with given RNG and message callback.
    pub fn new(r: &'self mut R, callback: &'self mut Listener) -> Preprocessor<'self,T,R,Listener> {
        let blocks = ~[Block { val: None, state: Outside, skip: false }];
        Preprocessor { blocks: blocks, r: r, callback: callback }
    }

    /// Returns true if any command which appears at this position should be ignored.
    pub fn inactive(&self) -> bool {
        let last = self.blocks.last();
        last.skip || last.state.inactive()
    }

    /// Adds the non-flow command (or any appropriate data) into the preprocessor.
    /// `result` will have zero or more preprocessed commands (or any appropriate data) inserted.
    pub fn feed_other(&mut self, cmd: T, result: &mut ~[T]) {
        if !self.inactive() {
            result.push(cmd);
        }
    }

    /// Adds the flow command into the preprocessor.
    /// `result` will have zero or more preprocessed commands (or any appropriate data) inserted.
    pub fn feed_flow(&mut self, lineno: Option<uint>, flow: &BmsFlowCommand, _result: &mut ~[T]) {
        let inactive = self.inactive();
        match *flow {
            BmsRandom(val) | BmsSetRandom(val) => {
                let val = if val <= 0 {None} else {Some(val)};
                let setrandom = match *flow { BmsSetRandom(*) => true, _ => false };

                // do not generate a random value if the entire block is skipped (but it
                // still marks the start of block)
                let generated = do val.and_then |val| {
                    if setrandom {
                        Some(val)
                    } else if !inactive {
                        Some(self.r.gen_integer_range(1, val + 1))
                    } else {
                        None
                    }
                };
                self.blocks.push(Block { val: generated, state: Outside, skip: inactive });
            }
            BmsEndRandom => {
                if self.blocks.len() > 1 { self.blocks.pop(); }
            }
            BmsIf(val) | BmsElseIf(val) => {
                let val = if val <= 0 {None} else {Some(val)};
                let haspriorelse = match *flow { BmsElseIf(*) => true, _ => false };

                let last = &mut self.blocks[self.blocks.len() - 1];
                last.state =
                    if (!haspriorelse && !last.state.inactive()) || last.state == Ignore {
                        if val.is_none() || val != last.val {Ignore} else {Process}
                    } else {
                        NoFurther
                    };
            }
            BmsElse => {
                let last = &mut self.blocks[self.blocks.len() - 1];
                last.state = if last.state == Ignore {Process} else {NoFurther};
            }
            BmsEndIf => {
                let lastinside = self.blocks.iter().rposition(|&i| i.state != Outside); // XXX #3511
                for &idx in lastinside.iter() {
                    if idx > 0 { self.blocks.truncate(idx + 1); }
                }

                let last = &mut self.blocks[self.blocks.len() - 1];
                last.state = Outside;
            }
            BmsSwitch(*) | BmsSetSwitch(*) | BmsEndSw | BmsCase(*) | BmsSkip | BmsDef => {
                self.callback.on_message(lineno, BmsHasUnimplementedFlow);
            }
        }
    }

    /// Terminates (and consumes) the preprocessor.
    /// `result` will have zero or more preprocessed commands (or any appropriate data) inserted.
    pub fn finish(self, _result: &mut ~[T]) {
    }
}

#[cfg(test)]
mod tests {
    use std::rand::rng;
    use format::bms::diag::BmsMessage;
    use super::Preprocessor;

    macro_rules! with_pp(
        ($pp:ident $blk:expr) => ({
            let mut r = rng();
            let mut callback: &fn(Option<uint>, BmsMessage) = |_, _| fail!("unexpected");
            let mut $pp = Preprocessor::new(&mut r, &mut callback);
            $blk;
        })
    )

    #[test]
    fn test_no_flow() {
        with_pp!(pp {
            let mut out = ~[];
            pp.feed_other(42, &mut out);
            assert_eq!(out.as_slice(), [42]);
            out.clear();
            pp.finish(&mut out);
            assert_eq!(out.as_slice(), []);
        })
    }

    // TODO add more tests
}

