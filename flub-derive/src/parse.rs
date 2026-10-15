use core::{
   iter::Peekable,
   str::FromStr,
};

use proc_macro::{
   Delimiter,
   Group,
   Ident,
   Literal,
   Span,
   TokenStream,
   TokenTree,
};

use crate::Fault;

/// The struct or enum under the derive.
pub struct Item {
   /// The type name.
   pub ident:    Ident,
   /// Generic parameters and where clause.
   pub generics: Generics,
   /// The type-level `#[error(...)]`, the fallback for enum variants.
   pub display:  Option<Display>,
   /// The fields or variants.
   pub body:     Body,
}

/// Fields of a struct or variants of an enum.
pub enum Body {
   /// A struct of any shape.
   Struct(Fields),
   /// An enum, possibly empty.
   Enum(Vec<Variant>),
}

/// One enum variant.
pub struct Variant {
   /// The variant name.
   pub ident:   Ident,
   /// The variant's own `#[error(...)]`, falling back to the enum's.
   pub display: Option<Display>,
   /// The variant's fields.
   pub fields:  Fields,
}

/// The fields of a struct or variant.
pub struct Fields {
   /// Braced fields with names, as opposed to tuple or unit fields.
   pub named: bool,
   /// Fields in declaration order.
   pub list:  Vec<Field>,
}

/// One field and the attributes flub reads from it.
pub struct Field {
   /// The field name, or its index for tuple fields.
   pub member:    TokenTree,
   /// The local this field is bound to when the item is destructured.
   pub binding:   Ident,
   /// The field type as written.
   pub ty:        Vec<TokenTree>,
   /// The span of `#[source]`, if present.
   pub source:    Option<Span>,
   /// The span of `#[from]`, if present.
   pub from:      Option<Span>,
   /// Whether `#[backtrace]` is present.
   pub backtrace: bool,
}

/// The argument of `#[error(...)]`.
pub enum Display {
   /// `#[error(transparent)]`, which forwards to the only field.
   Transparent(Span),
   /// `#[error(fmt = path)]`, which calls `path(&field, .., formatter)`.
   Function(Vec<TokenTree>),
   /// A format string with optional arguments.
   Format {
      /// The format string literal as written.
      literal:   Literal,
      /// Tokens after the format string's comma.
      arguments: TokenStream,
   },
}

/// Generic parameters and where clause of the item.
pub struct Generics {
   /// Parameters in declaration order.
   pub params:     Vec<Param>,
   /// Where-clause predicates without the `where` keyword.
   pub predicates: Vec<TokenTree>,
}

/// One generic parameter.
pub struct Param {
   /// The declaration with any default stripped, as an impl header needs it.
   pub declaration: Vec<TokenTree>,
   /// The parameter as it appears in the type's argument list.
   pub name:        Vec<TokenTree>,
   /// Whether this is a type parameter, as opposed to a lifetime or const.
   pub is_type:     bool,
}

/// The item keyword.
#[derive(Clone, Copy)]
enum ItemKind {
   /// `struct`.
   Struct,
   /// `enum`.
   Enum,
}

impl FromStr for ItemKind {
   type Err = ();

   fn from_str(keyword: &str) -> Result<Self, ()> {
      match keyword {
         "struct" => Ok(Self::Struct),
         "enum" => Ok(Self::Enum),
         _ => Err(()),
      }
   }
}

/// An attribute flub reads. Other attributes pass through untouched.
#[derive(Clone, Copy)]
enum AttributeName {
   /// `#[error(...)]`.
   Error,
   /// `#[source]`.
   Source,
   /// `#[from]`.
   From,
   /// `#[backtrace]`.
   Backtrace,
}

impl FromStr for AttributeName {
   type Err = ();

   fn from_str(name: &str) -> Result<Self, ()> {
      match name {
         "error" => Ok(Self::Error),
         "source" => Ok(Self::Source),
         "from" => Ok(Self::From),
         "backtrace" => Ok(Self::Backtrace),
         _ => Err(()),
      }
   }
}

/// Attributes flub reads from an item, variant, or field.
#[derive(Default)]
struct Attributes {
   /// `#[error(...)]`.
   display:   Option<Display>,
   /// `#[source]`.
   source:    Option<Span>,
   /// `#[from]`.
   from:      Option<Span>,
   /// `#[backtrace]`.
   backtrace: bool,
}

/// Classifying a single token.
pub trait TokenExt {
   /// The identifier this token is, if any.
   fn as_ident(&self) -> Option<&Ident>;
   /// The group this token is, if any.
   fn as_group(&self) -> Option<&Group>;
   /// Whether this token is the punctuation `ch`.
   fn is_punct(&self, ch: char) -> bool;
   /// Whether this token is the identifier `name`.
   fn is_keyword(&self, name: &str) -> bool;
   /// The single token inside an invisible group, which is how `macro_rules!`
   /// passes a `$fragment`, or the token itself.
   fn unwrap_invisible(self) -> Self;
}

impl TokenExt for TokenTree {
   fn as_ident(&self) -> Option<&Ident> {
      match *self {
         Self::Ident(ref ident) => Some(ident),
         Self::Group(_) | Self::Punct(_) | Self::Literal(_) => None,
      }
   }

   fn as_group(&self) -> Option<&Group> {
      match *self {
         Self::Group(ref group) => Some(group),
         Self::Ident(_) | Self::Punct(_) | Self::Literal(_) => None,
      }
   }

   fn is_punct(&self, ch: char) -> bool {
      matches!(*self, Self::Punct(ref punct) if punct.as_char() == ch)
   }

   fn is_keyword(&self, name: &str) -> bool {
      self
         .as_ident()
         .is_some_and(|ident| ident.to_string() == name)
   }

   fn unwrap_invisible(self) -> Self {
      if let Self::Group(ref group) = self
         && group.delimiter() == Delimiter::None
      {
         let mut inner = group.stream().into_iter();
         if let (Some(token), None) = (inner.next(), inner.next()) {
            return token;
         }
      }
      self
   }
}

/// Reading a token list front to back.
trait CursorExt {
   /// The span of the next token, or the call site at the end.
   fn span(&mut self) -> Span;
   /// Consumes the next token if it is the punctuation `ch`.
   fn eat_punct(&mut self, ch: char) -> bool;
   /// Consumes the next token if it is the identifier `name`.
   fn eat_keyword(&mut self, name: &str) -> bool;
   /// Consumes an identifier or reports one missing.
   fn ident(&mut self) -> Result<Ident, Fault>;
}

impl<I: Iterator<Item = TokenTree>> CursorExt for Peekable<I> {
   fn span(&mut self) -> Span {
      self.peek().map_or_else(Span::call_site, TokenTree::span)
   }

   fn eat_punct(&mut self, ch: char) -> bool {
      self.next_if(|token| token.is_punct(ch)).is_some()
   }

   fn eat_keyword(&mut self, name: &str) -> bool {
      self.next_if(|token| token.is_keyword(name)).is_some()
   }

   fn ident(&mut self) -> Result<Ident, Fault> {
      match self.next_if(|token| token.as_ident().is_some()) {
         Some(TokenTree::Ident(ident)) => Ok(ident),
         Some(_) | None => Err(Fault::new(self.span(), "expected an identifier")),
      }
   }
}

impl Fields {
   /// The field a placeholder or `.field` shorthand names, by index for tuple
   /// fields and by name, ignoring `r#`, for named ones.
   pub fn find(&self, name: &str) -> Option<&Field> {
      if self.named {
         return self.list.iter().find(|field| field.is_named(name));
      }
      self.list.get(name.parse::<usize>().ok()?)
   }
}

impl Field {
   /// Whether the field is named `name`, ignoring any `r#` prefix.
   pub fn is_named(&self, name: &str) -> bool {
      self.member.to_string().trim_start_matches("r#") == name
   }

   /// The type inside `Option<..>`, if this field is optional.
   pub fn option_inner(&self) -> Option<&[TokenTree]> {
      let open = self.ty.iter().position(|token| token.is_punct('<'))?;
      let close = self.ty.len().checked_sub(1)?;

      let wraps = self.ty.get(open.checked_sub(1)?)?.is_keyword("Option")
         && self.ty.get(close)?.is_punct('>');
      if !wraps {
         return None;
      }
      self.ty.get(open + 1..close)
   }

   /// Whether `#[from]` may fill this field with a captured backtrace, judged
   /// by the attribute or a last path segment of `Backtrace`.
   pub fn is_backtrace(&self) -> bool {
      self.backtrace
         || self
            .option_inner()
            .unwrap_or(&self.ty)
            .last()
            .is_some_and(|segment| segment.is_keyword("Backtrace"))
   }
}

impl Generics {
   /// Names of the type parameters, which decide whether a field type needs a
   /// bound.
   pub fn type_names(&self) -> Vec<String> {
      self
         .params
         .iter()
         .filter(|param| param.is_type)
         .flat_map(|param| &param.name)
         .map(ToString::to_string)
         .collect()
   }
}

/// Whether `ty` mentions any of `names`, searching into groups.
pub fn mentions(ty: &[TokenTree], names: &[String]) -> bool {
   ty.iter().any(|token| {
      match *token {
         TokenTree::Group(ref group) => {
            mentions(&group.stream().into_iter().collect::<Vec<_>>(), names)
         },
         TokenTree::Ident(ref ident) => names.contains(&ident.to_string()),
         TokenTree::Punct(_) | TokenTree::Literal(_) => false,
      }
   })
}

/// Splits on top-level commas. With `track_angles`, commas inside `<..>` stay
/// put, so `HashMap<K, V>` is one chunk.
fn split_commas(tokens: TokenStream, track_angles: bool) -> Vec<Vec<TokenTree>> {
   let mut chunks = vec![Vec::new()];
   let mut depth = 0_usize;
   let mut after_dash = false;

   for token in tokens {
      if let TokenTree::Punct(ref punct) = token {
         match punct.as_char() {
            ',' if depth == 0 => {
               chunks.push(Vec::new());
               after_dash = false;
               continue;
            },
            '<' if track_angles => depth += 1,
            '>' if track_angles && !after_dash => depth = depth.saturating_sub(1),
            _ => {},
         }
      }

      after_dash = token.is_punct('-');
      if let Some(chunk) = chunks.last_mut() {
         chunk.push(token);
      }
   }

   chunks.retain(|chunk| !chunk.is_empty());
   chunks
}

/// Parses the derive input.
pub fn item(input: TokenStream) -> Result<Item, Fault> {
   let mut cursor = input.into_iter().peekable();
   let display = attributes(&mut cursor)?.display;
   visibility(&mut cursor);

   let keyword = cursor.ident()?;
   let ident = cursor.ident()?;
   let params = if cursor.eat_punct('<') {
      params(&mut cursor)?
   } else {
      Vec::new()
   };
   let mut rest: Vec<TokenTree> = cursor.collect();

   let Ok(kind) = keyword.to_string().parse::<ItemKind>() else {
      return Err(Fault::new(
         keyword.span(),
         "Error can only be derived for structs and enums",
      ));
   };

   let body = match kind {
      ItemKind::Struct => {
         if let Some(group) = rest.first().and_then(TokenExt::as_group)
            && group.delimiter() == Delimiter::Parenthesis
         {
            let tuple = fields(group)?;
            rest.remove(0);
            if rest.last().is_some_and(|token| token.is_punct(';')) {
               rest.pop();
            }
            Body::Struct(tuple)
         } else if let Some(TokenTree::Group(group)) = rest.pop() {
            Body::Struct(fields(&group)?)
         } else {
            Body::Struct(Fields {
               named: false,
               list:  Vec::new(),
            })
         }
      },
      ItemKind::Enum => {
         let Some(TokenTree::Group(group)) = rest.pop() else {
            return Err(Fault::new(ident.span(), "expected enum body"));
         };
         Body::Enum(variants(&group)?)
      },
   };

   if rest.first().is_some_and(|token| token.is_keyword("where")) {
      rest.remove(0);
   }

   Ok(Item {
      ident,
      generics: Generics {
         params,
         predicates: rest,
      },
      display,
      body,
   })
}

/// Reads outer attributes, keeping the ones flub owns.
fn attributes(cursor: &mut Peekable<impl Iterator<Item = TokenTree>>) -> Result<Attributes, Fault> {
   let mut found = Attributes::default();

   while cursor.eat_punct('#') {
      let Some(TokenTree::Group(group)) = cursor.next() else {
         return Err(Fault::new(cursor.span(), "expected attribute"));
      };

      let mut inner = group.stream().into_iter();
      let Some(TokenTree::Ident(name)) = inner.next() else {
         continue;
      };
      let Ok(attribute) = name.to_string().parse::<AttributeName>() else {
         continue;
      };

      match attribute {
         AttributeName::Error => {
            if found.display.is_some() {
               return Err(Fault::new(name.span(), "duplicate #[error] attribute"));
            }
            let Some(TokenTree::Group(arguments)) = inner.next() else {
               return Err(Fault::new(name.span(), "expected #[error(...)]"));
            };
            found.display = Some(display(&arguments)?);
         },
         AttributeName::Source => found.source = Some(name.span()),
         AttributeName::From => found.from = Some(name.span()),
         AttributeName::Backtrace => found.backtrace = true,
      }
   }

   Ok(found)
}

/// Parses the inside of `#[error(...)]`.
fn display(group: &Group) -> Result<Display, Fault> {
   let mut cursor = group.stream().into_iter().peekable();

   match cursor.next().map(TokenExt::unwrap_invisible) {
      Some(token) if token.is_keyword("transparent") && cursor.peek().is_none() => {
         Ok(Display::Transparent(token.span()))
      },
      Some(token) if token.is_keyword("fmt") && cursor.eat_punct('=') => {
         let path: Vec<TokenTree> = cursor.collect();
         if path.is_empty() {
            return Err(Fault::new(token.span(), "expected a path after `fmt =`"));
         }
         Ok(Display::Function(path))
      },
      Some(TokenTree::Literal(literal)) => {
         if cursor.peek().is_some() && !cursor.eat_punct(',') {
            return Err(Fault::new(cursor.span(), "expected `,`"));
         }
         Ok(Display::Format {
            literal,
            arguments: cursor.collect(),
         })
      },
      Some(_) | None => {
         Err(Fault::new(
            group.span(),
            "expected a format string, `transparent`, or `fmt = path`",
         ))
      },
   }
}

/// Skips `pub`, `pub(crate)`, and the other restricted forms.
fn visibility(cursor: &mut Peekable<impl Iterator<Item = TokenTree>>) {
   let fragment = cursor.next_if(|token| {
      token.as_group().is_some_and(|group| {
         group.delimiter() == Delimiter::None
            && group
               .stream()
               .into_iter()
               .next()
               .is_none_or(|first| first.is_keyword("pub"))
      })
   });
   if fragment.is_some() || !cursor.eat_keyword("pub") {
      return;
   }

   cursor.next_if(|token| {
      token.as_group().is_some_and(|group| {
         group.delimiter() == Delimiter::Parenthesis
            && group.stream().into_iter().next().is_some_and(|first| {
               ["crate", "self", "super", "in"]
                  .iter()
                  .any(|keyword| first.is_keyword(keyword))
            })
      })
   });
}

/// Reads generic parameters up to the closing `>`, skipping the `>` of an `->`
/// in bounds like `F: Fn() -> T`.
fn params(cursor: &mut Peekable<impl Iterator<Item = TokenTree>>) -> Result<Vec<Param>, Fault> {
   let mut tokens = Vec::new();
   let mut depth = 0_usize;

   loop {
      let Some(token) = cursor.next() else {
         return Err(Fault::new(Span::call_site(), "unclosed generics"));
      };

      let arrow = tokens
         .last()
         .is_some_and(|last: &TokenTree| last.is_punct('-'));
      if token.is_punct('<') {
         depth += 1;
      } else if token.is_punct('>') && !arrow {
         let Some(outer) = depth.checked_sub(1) else {
            break;
         };
         depth = outer;
      }

      tokens.push(token);
   }

   Ok(split_commas(tokens.into_iter().collect(), true)
      .into_iter()
      .map(param)
      .collect())
}

/// Splits one generic parameter into its declaration and its name.
fn param(mut tokens: Vec<TokenTree>) -> Param {
   let mut depth = 0_usize;
   let default = tokens.iter().position(|token| {
      if token.is_punct('<') {
         depth += 1;
      } else if token.is_punct('>') {
         depth = depth.saturating_sub(1);
      }
      depth == 0 && token.is_punct('=')
   });

   if let Some(equals) = default {
      tokens.truncate(equals);
   }

   let mut unattributed = tokens.as_slice();
   while let [ref pound, TokenTree::Group(_), ref rest @ ..] = *unattributed
      && pound.is_punct('#')
   {
      unattributed = rest;
   }

   let (name, is_type) = match *unattributed {
      [ref tick, ref lifetime, ..] if tick.is_punct('\'') => {
         (vec![tick.clone(), lifetime.clone()], false)
      },
      [ref keyword, ref constant, ..] if keyword.is_keyword("const") => {
         (vec![constant.clone()], false)
      },
      [ref ident, ..] => (vec![ident.clone()], true),
      [] => (Vec::new(), true),
   };

   Param {
      declaration: tokens,
      name,
      is_type,
   }
}

/// Parses a braced or parenthesized field list.
fn fields(group: &Group) -> Result<Fields, Fault> {
   let named = group.delimiter() == Delimiter::Brace;
   let mut list = Vec::new();

   for (index, chunk) in split_commas(group.stream(), true).into_iter().enumerate() {
      let mut cursor = chunk.into_iter().peekable();
      let found = attributes(&mut cursor)?;
      visibility(&mut cursor);

      let member = if named {
         let ident = cursor.ident()?;
         if !cursor.eat_punct(':') {
            return Err(Fault::new(cursor.span(), "expected `:`"));
         }
         TokenTree::Ident(ident)
      } else {
         let mut position = Literal::usize_unsuffixed(index);
         position.set_span(cursor.span());
         TokenTree::Literal(position)
      };

      let name = member.to_string();
      let binding = Ident::new(
         &format!("__flub_{}", name.trim_start_matches("r#")),
         Span::mixed_site(),
      );

      list.push(Field {
         member,
         binding,
         ty: cursor.collect(),
         source: found.source,
         from: found.from,
         backtrace: found.backtrace,
      });
   }

   Ok(Fields { named, list })
}

/// Parses the variants of an enum body. Commas split without angle tracking,
/// since a discriminant like `1 << 2` would otherwise swallow the variants
/// after it.
fn variants(group: &Group) -> Result<Vec<Variant>, Fault> {
   split_commas(group.stream(), false)
      .into_iter()
      .map(|chunk| {
         let mut cursor = chunk.into_iter().peekable();
         let display = attributes(&mut cursor)?.display;
         let ident = cursor.ident()?;

         let body = cursor
            .peek()
            .and_then(TokenExt::as_group)
            .filter(|candidate| candidate.delimiter() != Delimiter::Bracket);
         let list = match body {
            Some(braced) => fields(braced)?,
            None => {
               Fields {
                  named: false,
                  list:  Vec::new(),
               }
            },
         };

         Ok(Variant {
            ident,
            display,
            fields: list,
         })
      })
      .collect()
}
