//! A zero-dependency `#[derive(Error)]` that writes the
//! [`Error`](core::error::Error), [`Display`](core::fmt::Display), and
//! [`From`] impls for your error types. See the [`Error`] derive.

#![no_std]
#![expect(
   clippy::pub_use,
   reason = "a proc-macro crate cannot export the runtime traits"
)]

#[cfg(feature = "std")] extern crate std;

pub use flub_derive::Error;

#[doc(hidden)] pub mod __private;

#[cfg(doctest)] mod compile_fail;
