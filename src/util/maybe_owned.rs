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
use std::cmp::{PartialEq, Eq, PartialOrd, Ord, Equiv};
use std::cmp::Ordering;
use std::default::Default;

/// A vector that can hold either `&'r [T]` or `Vec<T>`.
pub enum MaybeOwnedVec<'r, T:'r> {
    Owned(Vec<T>),
    Slice(&'r [T]),
}

impl<'r,T> MaybeOwnedVec<'r,T> {
    /// Returns `true` if the vector is owned.
    #[inline]
    pub fn is_owned(&self) -> bool {
        match *self {
            MaybeOwnedVec::Owned(..) => true,
            MaybeOwnedVec::Slice(..) => false,
        }
    }

    /// Returns `true` if the vector is borrowed.
    #[inline]
    pub fn is_slice(&self) -> bool {
        match *self {
            MaybeOwnedVec::Owned(..) => false,
            MaybeOwnedVec::Slice(..) => true,
        }
    }

    /// Returns the length of vector.
    #[inline]
    pub fn len(&self) -> uint { self.as_slice().len() }
}

/// A trait for moving into an `MaybeOwnedVec`.
pub trait IntoMaybeOwnedVec<'r,T> {
    /// Moves `self` into an `MaybeOwnedVec`.
    fn into_maybe_owned_vec(self) -> MaybeOwnedVec<'r,T>;
}

impl<T> IntoMaybeOwnedVec<'static,T> for Vec<T> {
    #[inline]
    fn into_maybe_owned_vec(self) -> MaybeOwnedVec<'static,T> { MaybeOwnedVec::Owned(self) }
}

impl<'r,T> IntoMaybeOwnedVec<'r,T> for &'r [T] {
    #[inline]
    fn into_maybe_owned_vec(self) -> MaybeOwnedVec<'r,T> { MaybeOwnedVec::Slice(self) }
}

impl<'r,T> AsSlice<T> for MaybeOwnedVec<'r,T> {
    #[inline]
    fn as_slice<'r>(&'r self) -> &'r [T] {
        match *self {
            MaybeOwnedVec::Owned(ref v) => v[],
            MaybeOwnedVec::Slice(v) => v,
        }
    }
}

impl<'r,T:fmt::Show> fmt::Show for MaybeOwnedVec<'r,T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { self.as_slice().fmt(f) }
}

impl<'r,T:PartialEq> PartialEq for MaybeOwnedVec<'r,T> {
    #[inline]
    fn eq(&self, other: &MaybeOwnedVec<'r,T>) -> bool {
        self.as_slice().eq(other.as_slice())
    }
}

impl<'r,T:Eq> Eq for MaybeOwnedVec<'r,T> {}

impl<'r,T:PartialEq,V:AsSlice<T>> Equiv<V> for MaybeOwnedVec<'r,T> {
    #[inline]
    fn equiv(&self, other: &V) -> bool {
        self.as_slice().eq(other.as_slice())
    }
}

impl<'r,T:PartialOrd> PartialOrd for MaybeOwnedVec<'r,T> {
    #[inline]
    fn partial_cmp(&self, other: &MaybeOwnedVec<'r,T>) -> Option<Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}

impl<'r,T:Ord> Ord for MaybeOwnedVec<'r,T> {
    #[inline]
    fn cmp(&self, other: &MaybeOwnedVec<'r,T>) -> Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl<'r,T:Clone> Clone for MaybeOwnedVec<'r,T> {
    #[inline]
    fn clone(&self) -> MaybeOwnedVec<'r,T> {
        match *self {
            MaybeOwnedVec::Owned(ref v) => MaybeOwnedVec::Owned(v.clone()),
            MaybeOwnedVec::Slice(v) => MaybeOwnedVec::Slice(v),
        }
    }
}

impl<T> Default for MaybeOwnedVec<'static,T> {
    #[inline]
    fn default() -> MaybeOwnedVec<'static,T> { MaybeOwnedVec::Slice(&[]) }
}

impl<'r,T:hash::Hash> hash::Hash for MaybeOwnedVec<'r,T> {
    #[inline]
    fn hash(&self, state: &mut hash::sip::SipState) { self.as_slice().hash(state) }
}

