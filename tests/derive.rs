//! Checks flub against thiserror on shared cases, plus the cases only flub
//! accepts.

#![cfg_attr(feature = "provide", feature(error_generic_member_access))]
#![expect(
   clippy::inline_modules,
   clippy::tests_outside_test_module,
   reason = "each derive needs its own module around the shared cases, and an integration test is \
             its own crate"
)]

use core::error::Error as StdError;

use flub::Error;

/// The shared cases derived by flub.
mod with_flub {
   use flub::Error;
   include!("cases/parity.rs");
}

/// The shared cases derived by thiserror.
mod with_thiserror {
   use thiserror::Error;
   include!("cases/parity.rs");
}

/// Renders the message, a padded message, and the source chain.
fn render(error: &dyn StdError) -> String {
   let mut out = format!("{error} |{error:>12}|");
   let mut next = error.source();
   while let Some(cause) = next {
      out = format!("{out} -> {cause}");
      next = cause.source();
   }
   out
}

#[test]
fn matches_thiserror() {
   let ours = with_flub::values();
   let theirs = with_thiserror::values();
   assert_eq!(ours.len(), theirs.len());

   for (left, right) in ours.iter().zip(&theirs) {
      assert_eq!(render(&**left), render(&**right));
   }
}

/// A leaf cause.
#[derive(Error, Debug)]
#[error("leaf")]
struct Leaf;

/// An alias that hides `Option` from a syntactic check.
type Maybe<T> = Option<T>;

/// Causes that thiserror rejects.
#[derive(Error, Debug)]
enum Beyond {
   /// An `Option` behind an alias.
   #[error("aliased")]
   Aliased(#[source] Maybe<Leaf>),
   /// An `Option` of a report that only derefs to `dyn Error`.
   #[error("report")]
   OptionalReport(#[source] Option<with_flub::Report>),
}

#[test]
fn reaches_hidden_sources() {
   let cases = [
      Beyond::Aliased(Some(Leaf)),
      Beyond::Aliased(None),
      Beyond::OptionalReport(Some(with_flub::Report(Box::new(Leaf)))),
      Beyond::OptionalReport(None),
   ];
   let rendered = cases.iter().map(|error| render(error)).collect::<Vec<_>>();

   assert_eq!(rendered, [
      "aliased |aliased| -> leaf",
      "aliased |aliased|",
      "report |report| -> leaf",
      "report |report|",
   ]);
}
