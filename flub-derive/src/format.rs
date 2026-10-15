use core::fmt;

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

use crate::{
   Fault,
   parse::{
      Field,
      Fields,
      TokenExt as _,
   },
};

/// What the generated [`Display::fmt`](fmt::Display::fmt) body does.
pub enum Plan {
   /// No placeholders, so the text goes straight to
   /// [`write_str`](fmt::Formatter::write_str).
   Text(Literal),
   /// The tokens inside [`format_args!`].
   Format(Vec<TokenTree>),
}

/// A format string resolved against the fields it names.
pub struct Expansion<'field> {
   /// How to write the message.
   pub plan:     Plan,
   /// Fields paired with the formatting trait each placeholder requires.
   pub bounds:   Vec<(&'field Field, FormatTrait)>,
   /// Whether a bare `{field}` calls the `__flub_display` helper.
   pub displays: bool,
}

/// A [`core::fmt`] formatting trait, selected by a placeholder's spec.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FormatTrait {
   /// `{}`.
   Display,
   /// `{:?}`.
   Debug,
   /// `{:x}`.
   LowerHex,
   /// `{:X}`.
   UpperHex,
   /// `{:o}`.
   Octal,
   /// `{:b}`.
   Binary,
   /// `{:e}`.
   LowerExp,
   /// `{:E}`.
   UpperExp,
   /// `{:p}`.
   Pointer,
}

impl FormatTrait {
   /// The trait a spec selects by its last character.
   fn of(spec: Option<&str>) -> Self {
      match spec.and_then(|text| text.chars().next_back()) {
         Some('?') => Self::Debug,
         Some('x') => Self::LowerHex,
         Some('X') => Self::UpperHex,
         Some('o') => Self::Octal,
         Some('b') => Self::Binary,
         Some('e') => Self::LowerExp,
         Some('E') => Self::UpperExp,
         Some('p') => Self::Pointer,
         Some(_) | None => Self::Display,
      }
   }
}

impl fmt::Display for FormatTrait {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      let name = match *self {
         Self::Display => "Display",
         Self::Debug => "Debug",
         Self::LowerHex => "LowerHex",
         Self::UpperHex => "UpperHex",
         Self::Octal => "Octal",
         Self::Binary => "Binary",
         Self::LowerExp => "LowerExp",
         Self::UpperExp => "UpperExp",
         Self::Pointer => "Pointer",
      };
      write!(f, "::core::fmt::{name}")
   }
}

/// A placeholder whose argument names a field.
struct Hole<'field> {
   /// The field the placeholder names.
   field:  &'field Field,
   /// The text after `:`, if any.
   spec:   Option<&'field str>,
   /// The formatting trait `spec` selects.
   format: FormatTrait,
}

impl Hole<'_> {
   /// Whether a bare `{field}` goes through `__flub_display`, which turns a
   /// [`Path`](::std::path::Path) or [`PathBuf`](::std::path::PathBuf) into its
   /// [`Path::display`](::std::path::Path::display).
   const fn displays(&self) -> bool {
      self.spec.is_none()
   }

   /// The named argument this hole is rewritten to. `__flub_display` and `{:p}`
   /// bind something other than the field, so they get their own names to
   /// coexist with a plain `{field:?}`.
   fn name(&self) -> String {
      let binding = &self.field.binding;
      if self.displays() {
         format!("{binding}_display")
      } else if self.format == FormatTrait::Pointer {
         format!("{binding}_pointer")
      } else {
         binding.to_string()
      }
   }

   /// The expression bound to the named argument. `{:p}` dereferences so it
   /// prints the field's pointer rather than the address of the binding.
   fn value(&self, span: Span) -> Vec<TokenTree> {
      let mut binding = self.field.binding.clone();
      binding.set_span(binding.span().located_at(span));
      let reference = TokenTree::Ident(binding);

      if self.displays() {
         let method = Ident::new("__flub_display", Span::call_site().located_at(span));
         vec![
            reference,
            punct('.'),
            TokenTree::Ident(method),
            TokenTree::Group(Group::new(Delimiter::Parenthesis, TokenStream::new())),
         ]
      } else if self.format == FormatTrait::Pointer {
         vec![punct('*'), reference]
      } else {
         vec![reference]
      }
   }
}

/// Rewrites the format string of `#[error(...)]` so every field it names
/// becomes an explicit named argument bound to the destructured field.
#[expect(
   clippy::string_slice,
   reason = "every slice boundary sits on an ASCII brace"
)]
pub fn expand<'field>(
   literal: &Literal,
   arguments: TokenStream,
   fields: &'field Fields,
) -> Result<Expansion<'field>, Fault> {
   let Some(content) = string_value(&literal.to_string()) else {
      return Err(Fault::new(literal.span(), "expected a string literal"));
   };

   let mut user_arguments = rewrite_members(arguments, fields)?;
   let user_named = named_arguments(&user_arguments);

   let mut rewritten = String::with_capacity(content.len());
   let mut text = String::with_capacity(content.len());
   let mut named: Vec<(String, Vec<TokenTree>)> = Vec::new();
   let mut bounds = Vec::new();
   let mut holes = 0_usize;
   let mut displays = false;
   let mut lone_brace = false;
   let mut rest = content.as_str();

   while let Some(brace) = rest.find(['{', '}']) {
      let (plain, tail) = rest.split_at(brace);
      rewritten.push_str(plain);
      text.push_str(plain);

      if tail.starts_with("{{") || tail.starts_with("}}") {
         rewritten.push_str(&tail[..2]);
         text.push_str(&tail[..1]);
         rest = &tail[2..];
         continue;
      }

      if let Some(after) = tail.strip_prefix('}') {
         lone_brace = true;
         rewritten.push('}');
         rest = after;
         continue;
      }

      let Some(end) = tail.find('}') else {
         rewritten.push_str(tail);
         rest = "";
         break;
      };

      let inner = &tail[1..end];
      rest = &tail[end + 1..];
      holes += 1;

      let (argument, spec) = match inner.split_once(':') {
         Some((name, after)) => (name, Some(after)),
         None => (inner, None),
      };

      let Some(field) = resolve(argument, fields, &user_named) else {
         rewritten.push('{');
         rewritten.push_str(inner);
         rewritten.push('}');
         continue;
      };

      let hole = Hole {
         field,
         spec,
         format: FormatTrait::of(spec),
      };
      bounds.push((field, hole.format));
      displays |= hole.displays();

      let name = hole.name();
      rewritten.push('{');
      rewritten.push_str(&name);
      if let Some(format_spec) = hole.spec {
         rewritten.push(':');
         rewritten.push_str(format_spec);
      }
      rewritten.push('}');

      if named.iter().all(|pair| pair.0 != name) {
         named.push((name, hole.value(literal.span())));
      }
   }

   rewritten.push_str(rest);
   text.push_str(rest);

   if holes == 0 && !lone_brace && user_arguments.is_empty() {
      return Ok(Expansion {
         plan: Plan::Text(relit(&text, literal.span())),
         bounds,
         displays,
      });
   }

   let mut tokens = vec![TokenTree::Literal(relit(&rewritten, literal.span()))];

   if user_arguments
      .last()
      .is_some_and(|token| token.is_punct(','))
   {
      user_arguments.pop();
   }
   if !user_arguments.is_empty() {
      tokens.push(punct(','));
      tokens.extend(user_arguments);
   }

   for (name, expression) in named {
      tokens.push(punct(','));
      tokens.push(TokenTree::Ident(Ident::new(&name, Span::call_site())));
      tokens.push(punct('='));
      tokens.extend(expression);
   }

   Ok(Expansion {
      plan: Plan::Format(tokens),
      bounds,
      displays,
   })
}

/// Unescapes a string or raw string literal, so `\u{7b}` counts as a `{` when
/// scanning for placeholders.
fn string_value(source: &str) -> Option<String> {
   if let Some(after) = source.strip_prefix('r') {
      let hashes = "#".repeat(after.len() - after.trim_start_matches('#').len());
      return after
         .strip_prefix(&hashes)?
         .strip_prefix('"')?
         .strip_suffix(&hashes)?
         .strip_suffix('"')
         .map(ToOwned::to_owned);
   }

   let body = source.strip_prefix('"')?.strip_suffix('"')?;
   let mut value = String::with_capacity(body.len());
   let mut chars = body.chars();

   while let Some(ch) = chars.next() {
      if ch != '\\' {
         value.push(ch);
         continue;
      }

      match chars.next()? {
         'n' => value.push('\n'),
         'r' => value.push('\r'),
         't' => value.push('\t'),
         '0' => value.push('\0'),
         escaped @ ('\\' | '\'' | '"') => value.push(escaped),
         'x' => {
            let digits = chars.as_str().get(..2)?;
            value.push(char::from(u8::from_str_radix(digits, 16).ok()?));
            chars.nth(1);
         },
         'u' => {
            let (digits, after) = chars.as_str().strip_prefix('{')?.split_once('}')?;
            let code = u32::from_str_radix(&digits.replace('_', ""), 16).ok()?;
            value.push(char::from_u32(code)?);
            chars = after.chars();
         },
         '\n' => {
            chars = chars
               .as_str()
               .trim_start_matches([' ', '\t', '\n', '\r'])
               .chars();
         },
         _ => return None,
      }
   }

   Some(value)
}

/// A string literal holding `value`, under the original span.
fn relit(value: &str, span: Span) -> Literal {
   let mut literal = Literal::string(value);
   literal.set_span(span);
   literal
}

/// The field a placeholder argument names, unless a user argument of that name
/// shadows it.
fn resolve<'field>(
   argument: &str,
   fields: &'field Fields,
   user_named: &[String],
) -> Option<&'field Field> {
   if fields.named && user_named.iter().any(|name| name == argument) {
      return None;
   }
   fields.find(argument)
}

/// A lone punctuation token.
fn punct(ch: char) -> TokenTree {
   TokenTree::Punct(Punct::new(ch, Spacing::Alone))
}

/// Names of `name = value` arguments the user wrote.
fn named_arguments(arguments: &[TokenTree]) -> Vec<String> {
   let mut names = Vec::new();
   let mut at_start = true;

   for (index, token) in arguments.iter().enumerate() {
      if at_start
         && let Some(ident) = token.as_ident()
         && arguments.get(index + 1).is_some_and(|next| {
            matches!(*next, TokenTree::Punct(ref equals) if equals.as_char() == '=' && equals.spacing() == Spacing::Alone)
         })
      {
         names.push(ident.to_string());
      }
      at_start = token.is_punct(',');
   }

   names
}

/// Replaces `.field` and `.0` shorthand in format arguments with the field
/// bindings, inside groups too. A dot is shorthand only where an expression
/// begins, so `.0.len()` keeps its method call and `.0.1` still reaches into
/// the tuple field.
fn rewrite_members(arguments: TokenStream, fields: &Fields) -> Result<Vec<TokenTree>, Fault> {
   let tokens: Vec<TokenTree> = arguments.into_iter().collect();
   let mut output = Vec::with_capacity(tokens.len());
   let mut begins = true;
   let mut index = 0;

   while let Some(token) = tokens.get(index) {
      index += 1;

      if begins
         && token.is_punct('.')
         && let Some(member) = tokens.get(index)
      {
         let text = member.to_string();
         let (position, nested) = match *member {
            TokenTree::Ident(_) => (text.as_str(), None),
            TokenTree::Literal(_) => {
               match text.split_once('.') {
                  Some((outer, inner)) => (outer, Some(inner)),
                  None => (text.as_str(), None),
               }
            },
            TokenTree::Group(_) | TokenTree::Punct(_) => ("", None),
         };

         if let Some(field) = shorthand_field(fields, member, position)? {
            let mut binding = field.binding.clone();
            binding.set_span(Span::mixed_site().located_at(member.span()));
            output.push(TokenTree::Ident(binding));

            if let Some(inner) = nested.and_then(|digits| digits.parse::<usize>().ok()) {
               let mut tuple_index = Literal::usize_unsuffixed(inner);
               tuple_index.set_span(member.span());
               output.push(punct('.'));
               output.push(TokenTree::Literal(tuple_index));
            }

            begins = false;
            index += 1;
            continue;
         }
      }

      begins = starts_expression(token);
      match *token {
         TokenTree::Group(ref group) => {
            let inner = rewrite_members(group.stream(), fields)?;
            let mut rebuilt = Group::new(group.delimiter(), inner.into_iter().collect());
            rebuilt.set_span(group.span());
            output.push(TokenTree::Group(rebuilt));
         },
         TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => {
            output.push(token.clone());
         },
      }
   }

   Ok(output)
}

/// The field a shorthand dot names, or [`None`] when the token after the dot is
/// not a member at all, like the second dot of `..`.
fn shorthand_field<'field>(
   fields: &'field Fields,
   member: &TokenTree,
   position: &str,
) -> Result<Option<&'field Field>, Fault> {
   let is_member = match *member {
      TokenTree::Ident(_) => true,
      TokenTree::Literal(_) => {
         !position.is_empty() && position.bytes().all(|byte| byte.is_ascii_digit())
      },
      TokenTree::Group(_) | TokenTree::Punct(_) => false,
   };
   if !is_member {
      return Ok(None);
   }

   fields
      .find(position)
      .map(Some)
      .ok_or_else(|| Fault::new(member.span(), format!("no field `{position}`")))
}

/// Whether an expression can begin right after `token`, which holds for
/// operators, commas, and keywords that take an expression.
fn starts_expression(token: &TokenTree) -> bool {
   match *token {
      TokenTree::Ident(ref ident) => {
         matches!(
            ident.to_string().as_str(),
            "break" | "continue" | "if" | "in" | "match" | "mut" | "return" | "while"
         )
      },
      TokenTree::Punct(ref punct) => {
         matches!(
            punct.as_char(),
            '+' | '&' | '!' | '^' | ',' | '/' | '=' | '>' | '<' | '|' | '%' | ';' | '*' | '-'
         )
      },
      TokenTree::Group(_) | TokenTree::Literal(_) => false,
   }
}
