//! A zero-dependency `#[derive(Error)]` that writes the
//! [`Error`](::core::error::Error), [`Display`](::core::fmt::Display), and
//! [`From`] impls for your error types.

/// Code generation for the derived impls.
mod expand;
/// Format string rewriting for `#[error("...")]`.
mod format;
/// Parsing the derive input from raw tokens.
mod parse;

use proc_macro::{
   Delimiter,
   Group,
   Ident,
   Literal,
   Punct,
   Spacing,
   Span,
   TokenStream,
   TokenTree,
};

/// Derives [`core::error::Error`], plus [`Display`](::core::fmt::Display) when
/// `#[error(...)]` is present and [`From`] for each `#[from]` field.
#[proc_macro_derive(Error, attributes(error, source, from, backtrace))]
#[inline]
pub fn derive_error(input: TokenStream) -> TokenStream {
   parse::item(input)
      .and_then(|item| expand::item(&item))
      .unwrap_or_else(TokenStream::from)
}

/// A diagnostic reported through [`compile_error!`] at the offending span.
struct Fault {
   /// The tokens rustc underlines.
   span:    Span,
   /// The text after `error:` in rustc's output.
   message: String,
}

impl Fault {
   /// A diagnostic at `span`.
   fn new(span: Span, message: impl Into<String>) -> Self {
      Self {
         span,
         message: message.into(),
      }
   }
}

impl From<Fault> for TokenStream {
   /// The [`compile_error!`] invocation that reports the fault.
   #[inline]
   fn from(value: Fault) -> Self {
      let span = value.span;

      let punct = |ch, spacing| {
         let mut punct = Punct::new(ch, spacing);
         punct.set_span(span);
         TokenTree::Punct(punct)
      };

      let mut message = Literal::string(&value.message);
      message.set_span(span);
      let mut arguments = Group::new(Delimiter::Parenthesis, TokenTree::Literal(message).into());
      arguments.set_span(span);

      [
         punct(':', Spacing::Joint),
         punct(':', Spacing::Alone),
         TokenTree::Ident(Ident::new("core", span)),
         punct(':', Spacing::Joint),
         punct(':', Spacing::Alone),
         TokenTree::Ident(Ident::new("compile_error", span)),
         punct('!', Spacing::Alone),
         TokenTree::Group(arguments),
         punct(';', Spacing::Alone),
      ]
      .into_iter()
      .collect()
   }
}
