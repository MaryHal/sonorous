// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, 2014, Kang Seonghoon.
// See README.md for details.
//
// Licensed under the Apache License, Version 2.0 <http://www.apache.org/licenses/LICENSE-2.0> or
// the MIT license <http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! Optionally owned containers, similar to `std::str::MaybeOwned`.

use std::{fmt, hash};
use std::clone::Clone;
use std::cmp::{Eq, TotalEq, Ord, TotalOrd, Equiv};
use std::cmp::Ordering;
use std::container::Container;
use std::default::Default;
use std::slice::Vector;

/// A vector that can hold either `&'r [T]` or `~[T]`.
pub enum MaybeOwnedVec<'r,T> {
    OwnedVec(~[T]),
    VecSlice(&'r [T]),
}

impl<'r,T> MaybeOwnedVec<'r,T> {
    /// Returns `true` if the vector is owned.
    #[inline]
    pub fn is_owned(&self) -> bool {
        match *self {
            OwnedVec(..) => true,
            VecSlice(..) => false,
        }
    }

    /// Returns `true` if the vector is borrowed.
    #[inline]
    pub fn is_slice(&self) -> bool {
        match *self {
            OwnedVec(..) => false,
            VecSlice(..) => true,
        }
    }
}

/// A trait for moving into an `MaybeOwnedVec`.
pub trait IntoMaybeOwnedVec<'r,T> {
    /// Moves `self` into an `MaybeOwnedVec`.
    fn into_maybe_owned_vec(self) -> MaybeOwnedVec<'r,T>;
}

impl<T> IntoMaybeOwnedVec<'static,T> for ~[T] {
    #[inline]
    fn into_maybe_owned_vec(self) -> MaybeOwnedVec<'static,T> { OwnedVec(self) }
}

impl<'r,T> IntoMaybeOwnedVec<'r,T> for &'r [T] {
    #[inline]
    fn into_maybe_owned_vec(self) -> MaybeOwnedVec<'r,T> { VecSlice(self) }
}

impl<'r,T> Vector<T> for MaybeOwnedVec<'r,T> {
    #[inline]
    fn as_slice<'r>(&'r self) -> &'r [T] {
        match *self {
            OwnedVec(ref v) => v.as_slice(),
            VecSlice(v) => v,
        }
    }
}

impl<'r,T:fmt::Show> fmt::Show for MaybeOwnedVec<'r,T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { self.as_slice().fmt(f) }
}

impl<'r,T:Eq> Eq for MaybeOwnedVec<'r,T> {
    #[inline]
    fn eq(&self, other: &MaybeOwnedVec<'r,T>) -> bool {
        self.as_slice().eq(&other.as_slice())
    }
}

impl<'r,T:TotalEq> TotalEq for MaybeOwnedVec<'r,T> {
    #[inline]
    fn equals(&self, other: &MaybeOwnedVec<'r,T>) -> bool {
        self.as_slice().equals(&other.as_slice())
    }
}

impl<'r,T:Eq,V:Vector<T>> Equiv<V> for MaybeOwnedVec<'r,T> {
    #[inline]
    fn equiv(&self, other: &V) -> bool {
        self.as_slice().eq(&other.as_slice())
    }
}

impl<'r,T:Eq+Ord> Ord for MaybeOwnedVec<'r,T> {
    #[inline]
    fn lt(&self, other: &MaybeOwnedVec<'r,T>) -> bool {
        self.as_slice().lt(&other.as_slice())
    }

    #[inline]
    fn le(&self, other: &MaybeOwnedVec<'r,T>) -> bool {
        self.as_slice().le(&other.as_slice())
    }

    #[inline]
    fn gt(&self, other: &MaybeOwnedVec<'r,T>) -> bool {
        self.as_slice().gt(&other.as_slice())
    }

    #[inline]
    fn ge(&self, other: &MaybeOwnedVec<'r,T>) -> bool {
        self.as_slice().ge(&other.as_slice())
    }
}

impl<'r,T:TotalOrd> TotalOrd for MaybeOwnedVec<'r,T> {
    #[inline]
    fn cmp(&self, other: &MaybeOwnedVec<'r,T>) -> Ordering {
        self.as_slice().cmp(&other.as_slice())
    }
}

impl<'r,T> Container for MaybeOwnedVec<'r,T> {
    #[inline]
    fn len(&self) -> uint { self.as_slice().len() }
}

impl<'r,T:Clone> Clone for MaybeOwnedVec<'r,T> {
    #[inline]
    fn clone(&self) -> MaybeOwnedVec<'r,T> {
        match *self {
            OwnedVec(ref v) => OwnedVec(v.clone()),
            VecSlice(v) => VecSlice(v),
        }
    }
}

impl<T> Default for MaybeOwnedVec<'static,T> {
    #[inline]
    fn default() -> MaybeOwnedVec<'static,T> { VecSlice(&'static []) }
}

impl<'r,T:hash::Hash> hash::Hash for MaybeOwnedVec<'r,T> {
    #[inline]
    fn hash(&self, state: &mut hash::sip::SipState) { self.as_slice().hash(state) }
}

