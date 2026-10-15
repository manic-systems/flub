//! Checks `Error::provide` forwarding under the `provide` feature.

#![cfg(feature = "provide")]
#![feature(error_generic_member_access)]
#![expect(
   clippy::tests_outside_test_module,
   reason = "an integration test is its own crate"
)]

use core::{
   error::{
      Error as StdError,
      request_ref,
   },
   ptr,
};
use std::{
   backtrace::Backtrace,
   io,
};

use flub::Error;

/// A cause holding its own backtrace.
#[derive(Error, Debug)]
#[error("leaf")]
struct Leaf {
   /// Captured at construction.
   backtrace: Backtrace,
}

/// Wraps causes with and without backtraces.
#[derive(Error, Debug)]
enum Mid {
   /// Forwards to the leaf.
   #[error("mid")]
   Wrap {
      /// The leaf.
      #[from]
      source: Leaf,
   },
   /// Offers its own backtrace since the cause has none.
   #[error("own")]
   Own {
      /// A cause without a backtrace.
      #[from]
      source:    io::Error,
      /// Filled by `From`.
      backtrace: Option<Backtrace>,
   },
   /// Forwards through a boxed cause.
   #[error(transparent)]
   Through(Box<dyn StdError + Send + Sync>),
   /// Has nothing to offer.
   #[error("plain")]
   Plain,
}

/// Forwards everything to `Mid`.
#[derive(Error, Debug)]
#[error(transparent)]
struct Top(#[from] Mid);

/// The leaf at the bottom of the source chain.
fn deepest(error: &dyn StdError) -> &dyn StdError {
   error.source().map_or(error, deepest)
}

/// Whether `error` provides the same backtrace as its deepest cause.
fn provides_deepest(error: &dyn StdError) -> bool {
   let ours = request_ref::<Backtrace>(error);
   let leaf = request_ref::<Backtrace>(deepest(error));
   ours
      .zip(leaf)
      .is_some_and(|(left, right)| ptr::eq(left, right))
}

#[test]
fn forwards_the_deepest_backtrace() {
   let leaf = || {
      Leaf {
         backtrace: Backtrace::disabled(),
      }
   };

   assert!(provides_deepest(&Top::from(Mid::from(leaf()))));
   assert!(provides_deepest(&Mid::Through(Box::new(leaf()))));
   assert!(request_ref::<Backtrace>(&Mid::from(io::Error::other("io"))).is_some());
   assert!(request_ref::<Backtrace>(&Mid::Plain).is_none());
}
