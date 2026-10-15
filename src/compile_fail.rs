//! Inputs the derive rejects with its own diagnostic.
//!
//! An enum needs `#[error(...)]` on every variant once one has it.
//!
//! ```compile_fail
//! #[derive(flub::Error, Debug)]
//! enum Partial {
//!    #[error("x")]
//!    X,
//!    Y,
//! }
//! ```
//!
//! `#[error(transparent)]` needs exactly one field.
//!
//! ```compile_fail
//! #[derive(flub::Error, Debug)]
//! #[error(transparent)]
//! struct Pair(std::io::Error, u8);
//! ```
//!
//! `#[from]` allows only a backtrace beside it.
//!
//! ```compile_fail
//! #[derive(flub::Error, Debug)]
//! #[error("x")]
//! struct Extra(#[from] std::io::Error, u8);
//! ```
//!
//! A `.field` shorthand must name a field.
//!
//! ```compile_fail
//! #[derive(flub::Error, Debug)]
//! #[error("{}", .nope)]
//! struct Missing {
//!    present: u8,
//! }
//! ```
