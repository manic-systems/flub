use core::{
   fmt,
   ptr,
};

use proc_macro::{
   Delimiter,
   Group,
   Ident,
   Punct,
   Spacing,
   Span,
   TokenStream,
   TokenTree,
};

use crate::{
   Fault,
   format::{
      self,
      FormatTrait,
      Plan,
   },
   parse::{
      Body,
      Display,
      Field,
      Fields,
      Item,
      TokenExt as _,
      mentions,
   },
};

/// Generated tokens. Fixed code is lexed from strings, user tokens are spliced
/// in with their spans intact.
#[derive(Default)]
struct Out(Vec<TokenTree>);

impl Out {
   /// Appends fixed source code.
   #[expect(
      clippy::expect_used,
      reason = "the fragments are fixed strings in this crate"
   )]
   fn code(&mut self, source: &str) {
      self
         .0
         .extend(source.parse::<TokenStream>().expect("fragment lexes"));
   }

   /// Appends user tokens.
   fn tokens<'tokens>(&mut self, tokens: impl IntoIterator<Item = &'tokens TokenTree>) {
      self.0.extend(tokens.into_iter().cloned());
   }

   /// Appends an identifier.
   fn ident(&mut self, ident: &Ident) {
      self.0.push(TokenTree::Ident(ident.clone()));
   }

   /// Appends a lone punctuation token.
   fn punct(&mut self, ch: char) {
      self
         .0
         .push(TokenTree::Punct(Punct::new(ch, Spacing::Alone)));
   }

   /// Appends `inner` wrapped in `delimiter`.
   fn group(&mut self, delimiter: Delimiter, inner: Self) {
      self
         .0
         .push(TokenTree::Group(Group::new(delimiter, inner.into())));
   }

   /// Whether nothing has been written.
   const fn is_empty(&self) -> bool {
      self.0.is_empty()
   }
}

impl Extend<TokenTree> for Out {
   fn extend<I>(&mut self, iter: I)
   where
      I: IntoIterator<Item = TokenTree>,
   {
      self.0.extend(iter);
   }
}

impl IntoIterator for Out {
   type IntoIter = <Vec<TokenTree> as IntoIterator>::IntoIter;
   type Item = TokenTree;

   fn into_iter(self) -> Self::IntoIter {
      self.0.into_iter()
   }
}

impl From<Out> for TokenStream {
   #[inline]
   fn from(value: Out) -> Self {
      value.0.into_iter().collect()
   }
}

/// What a generated bound requires of a field type.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Requirement {
   /// A formatting trait a placeholder uses.
   Format(FormatTrait),
   /// `Error + 'static`, for a source.
   Error,
}

impl fmt::Display for Requirement {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      match *self {
         Self::Format(format) => format.fmt(f),
         Self::Error => f.write_str("::core::error::Error + 'static"),
      }
   }
}

/// A where-clause predicate added for a field whose type mentions a type
/// parameter.
struct Bound<'item> {
   /// The field type.
   ty:          &'item [TokenTree],
   /// What the type must implement.
   requirement: Requirement,
}

/// How [`Error::source`](::core::error::Error::source) reaches a field's cause.
#[derive(Clone, Copy)]
enum Source<'item> {
   /// The field is the source, possibly through [`Option`] or
   /// [`Deref`](::core::ops::Deref).
   Field(&'item Field),
   /// The field's own source is the source, under `#[error(transparent)]`.
   Transparent(&'item Field),
}

impl<'item> Source<'item> {
   /// The field holding the cause.
   const fn field(self) -> &'item Field {
      match self {
         Self::Field(field) | Self::Transparent(field) => field,
      }
   }

   /// The type a generic bound requires to implement
   /// [`Error`](::core::error::Error), unwrapping the field type when it's
   /// spelled `Option<..>`.
   fn error_type(self) -> &'item [TokenTree] {
      match self {
         Self::Field(field) => field.option_inner().unwrap_or(&field.ty),
         Self::Transparent(field) => &field.ty,
      }
   }
}

/// One struct, or one enum variant, with the display it resolved to.
struct Case<'item> {
   /// The variant name, absent for a struct.
   variant: Option<&'item Ident>,
   /// The fields.
   fields:  &'item Fields,
   /// The `#[error(...)]` that applies.
   display: Option<&'item Display>,
}

/// Generates the [`Display`](::core::fmt::Display),
/// [`Error`](::core::error::Error), and [`From`] impls for `item`.
pub fn item(item: &Item) -> Result<TokenStream, Fault> {
   let cases: Vec<Case<'_>> = match item.body {
      Body::Struct(ref fields) => {
         vec![Case {
            variant: None,
            fields,
            display: item.display.as_ref(),
         }]
      },
      Body::Enum(ref variants) => {
         if let Some(&Display::Transparent(span)) = item.display.as_ref() {
            return Err(Fault::new(
               span,
               "#[error(transparent)] belongs on a variant",
            ));
         }

         variants
            .iter()
            .map(|variant| {
               Case {
                  variant: Some(&variant.ident),
                  fields:  &variant.fields,
                  display: variant.display.as_ref().or(item.display.as_ref()),
               }
            })
            .collect()
      },
   };

   let type_params = item.generics.type_names();

   let mut out = Out::default();
   display_impl(&mut out, item, &cases, &type_params)?;
   error_impl(&mut out, item, &cases, &type_params)?;

   for case in &cases {
      for field in &case.fields.list {
         from_impl(&mut out, item, case, field)?;
      }
   }

   Ok(out.into())
}

/// Writes `impl Display`, or nothing when no case has `#[error(...)]`, leaving
/// room for a hand-written impl.
fn display_impl(
   out: &mut Out,
   item: &Item,
   cases: &[Case<'_>],
   type_params: &[String],
) -> Result<(), Fault> {
   if !cases.is_empty() && cases.iter().all(|case| case.display.is_none()) {
      return Ok(());
   }

   let mut bounds = Vec::new();
   let mut arms = Out::default();
   let mut displays = false;

   for case in cases {
      let Some(display) = case.display else {
         let span = case.variant.map_or_else(|| item.ident.span(), Ident::span);
         return Err(Fault::new(span, "missing #[error(...)] on this variant"));
      };

      let bindings = case.fields.list.iter().map(|field| (field, &field.binding));
      pattern(&mut arms, case.variant, bindings, false);
      arms.code("=>");
      displays |= display_body(&mut arms, display, case.fields, &mut bounds)?;
      arms.punct(',');
   }

   let mut body = Out::default();
   if displays {
      body.code(DISPLAY_HELPER);
   }
   body.code(if cases.is_empty() {
      "match *self"
   } else {
      "match self"
   });
   body.group(Delimiter::Brace, arms);

   let mut function = Out::default();
   function.code(
      "fn fmt(&self, __flub_formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result",
   );
   function.group(Delimiter::Brace, body);

   let mut trait_path = Out::default();
   trait_path.code("::core::fmt::Display");
   header(out, item, trait_path, type_params, &bounds, false);
   out.group(Delimiter::Brace, function);
   Ok(())
}

/// Writes `impl Error`, with a `source` method when any case has a cause.
fn error_impl(
   out: &mut Out,
   item: &Item,
   cases: &[Case<'_>],
   type_params: &[String],
) -> Result<(), Fault> {
   let sources = cases
      .iter()
      .map(|case| {
         let transparent = matches!(case.display, Some(&Display::Transparent(_)));
         source(case.fields, transparent)
      })
      .collect::<Result<Vec<_>, _>>()?;

   let mut bounds = Vec::new();
   let mut arms = Out::default();
   let mut sourceless = false;

   for (case, entry) in cases.iter().zip(&sources) {
      let Some(found) = *entry else {
         sourceless = true;
         continue;
      };

      let binding = Ident::new("__flub_source", Span::mixed_site());
      pattern(&mut arms, case.variant, [(found.field(), &binding)], true);
      arms.code("=>");
      source_expression(&mut arms, found, &binding);
      arms.punct(',');

      bounds.push(Bound {
         ty:          found.error_type(),
         requirement: Requirement::Error,
      });
   }

   let mut methods = Out::default();
   if !arms.is_empty() {
      if sourceless {
         arms.code("_ => ::core::option::Option::None,");
      }

      let mut body = Out::default();
      body.code(SOURCE_HELPER);
      body.code("match self");
      body.group(Delimiter::Brace, arms);

      methods
         .code("fn source(&self) -> ::core::option::Option<&(dyn ::core::error::Error + 'static)>");
      methods.group(Delimiter::Brace, body);
   }

   #[cfg(feature = "provide")]
   provide_method(&mut methods, cases, &sources);

   let mut trait_path = Out::default();
   trait_path.code("::core::error::Error");
   header(out, item, trait_path, type_params, &bounds, true);
   out.group(Delimiter::Brace, methods);
   Ok(())
}

/// Writes [`provide`](::core::error::Error::provide), which forwards to each
/// case's source first, so the deepest backtrace wins, then offers the case's
/// own backtrace.
#[cfg(feature = "provide")]
fn provide_method(methods: &mut Out, cases: &[Case<'_>], sources: &[Option<Source<'_>>]) {
   let source_binding = Ident::new("__flub_source", Span::mixed_site());
   let backtrace_binding = Ident::new("__flub_backtrace", Span::mixed_site());
   let mut arms = Out::default();
   let mut silent = false;

   for (case, &found) in cases.iter().zip(sources) {
      let backtrace = match found {
         Some(Source::Transparent(_)) => None,
         Some(Source::Field(_)) | None => {
            let list = &case.fields.list;
            list
               .iter()
               .find(|field| field.backtrace)
               .or_else(|| list.iter().find(|field| field.is_backtrace()))
         },
      };

      let own_backtrace =
         backtrace.filter(|field| found.is_none_or(|source| !ptr::eq(source.field(), *field)));

      if found.is_none() && own_backtrace.is_none() {
         silent = true;
         continue;
      }

      let mut bindings = Vec::new();
      if let Some(source) = found {
         bindings.push((source.field(), &source_binding));
      }
      if let Some(field) = own_backtrace {
         bindings.push((field, &backtrace_binding));
      }
      pattern(&mut arms, case.variant, bindings, true);
      arms.code("=>");

      let mut body = Out::default();
      if let Some(source) = found {
         let mut call = Out::default();
         call.code("::core::error::Error::provide(__flub_cause, __flub_request);");

         body.code("if let ::core::option::Option::Some(__flub_cause) =");
         source_call(&mut body, &source_binding, source.field());
         body.group(Delimiter::Brace, call);
      }
      if let Some(field) = own_backtrace {
         let mut call = Out::default();
         call.code("::core::error::Request::provide_ref::<::std::backtrace::Backtrace>");
         let mut arguments = Out::default();
         arguments.code("__flub_request,");
         arguments.ident(&backtrace_binding);
         call.group(Delimiter::Parenthesis, arguments);
         call.punct(';');
         unwrap_option(
            &mut body,
            &backtrace_binding,
            field.option_inner().is_some(),
            call,
         );
      }
      arms.group(Delimiter::Brace, body);
   }

   if arms.is_empty() {
      return;
   }
   if silent {
      arms.code("_ => {}");
   }

   let mut body = Out::default();
   body.code(SOURCE_HELPER);
   body.code("match self");
   body.group(Delimiter::Brace, arms);

   methods.code(
      "fn provide<'__flub>(&'__flub self, __flub_request: &mut ::core::error::Request<'__flub>)",
   );
   methods.group(Delimiter::Brace, body);
}

/// Writes `call`, inside `if let Some(binding) = binding` when the backtrace
/// field is an [`Option`].
#[cfg(feature = "provide")]
fn unwrap_option(out: &mut Out, binding: &Ident, optional: bool, call: Out) {
   if !optional {
      out.extend(call);
      return;
   }

   let mut inner = Out::default();
   inner.ident(binding);
   out.code("if let ::core::option::Option::Some");
   out.group(Delimiter::Parenthesis, inner);
   out.punct('=');
   out.ident(binding);
   out.group(Delimiter::Brace, call);
}

/// Brings the `__flub_source` traits into scope. They share the method name,
/// so autoref picks the first that applies, an error or `dyn Error`, then an
/// [`Option`] of one, then an [`Option`] of a pointer like `Box<dyn Error>`.
const SOURCE_HELPER: &str =
   "use ::flub::__private::{OptionSource as _, PointerSource as _, Source as _};";

/// Brings `__flub_display` into scope for bare `{field}` placeholders.
const DISPLAY_HELPER: &str = "use ::flub::__private::AsDisplay as _;";

/// Writes an impl header through the where clause. Bounds only land for field
/// types that mention a type parameter, since the rest hold or fail regardless.
fn header(
   out: &mut Out,
   item: &Item,
   trait_path: Out,
   type_params: &[String],
   bounds: &[Bound<'_>],
   error: bool,
) {
   let generics = &item.generics;
   out.code("#[automatically_derived] #[allow(unused_qualifications)] impl");

   if !generics.params.is_empty() {
      out.punct('<');
      for param in &generics.params {
         out.tokens(&param.declaration);
         out.punct(',');
      }
      out.punct('>');
   }

   out.extend(trait_path);
   out.code("for");
   out.ident(&item.ident);

   if !generics.params.is_empty() {
      out.punct('<');
      for param in &generics.params {
         out.tokens(&param.name);
         out.punct(',');
      }
      out.punct('>');
   }

   let mut generic_bounds: Vec<(String, &Bound<'_>)> = Vec::new();
   for bound in bounds
      .iter()
      .filter(|bound| mentions(bound.ty, type_params))
   {
      let key = bound
         .ty
         .iter()
         .cloned()
         .collect::<TokenStream>()
         .to_string();
      if !generic_bounds
         .iter()
         .any(|&(ref seen, other)| *seen == key && other.requirement == bound.requirement)
      {
         generic_bounds.push((key, bound));
      }
   }

   let self_bound = error && !generics.params.is_empty();
   if generics.predicates.is_empty() && generic_bounds.is_empty() && !self_bound {
      return;
   }

   out.code("where");
   out.tokens(&generics.predicates);
   if generics
      .predicates
      .last()
      .is_some_and(|token| !token.is_punct(','))
   {
      out.punct(',');
   }

   for (_, bound) in generic_bounds {
      out.extend(bound.ty.iter().map(expansion_token));
      out.punct(':');
      out.code(&bound.requirement.to_string());
      out.punct(',');
   }

   if self_bound {
      out.code("Self: ::core::fmt::Debug + ::core::fmt::Display,");
   }
}

/// Copies a user token into the expansion's hygiene context, keeping its
/// location, so lints on generated bounds such as clippy's
/// `trait_duplication_in_bounds` don't land on the user's own `T: Debug`.
fn expansion_token(token: &TokenTree) -> TokenTree {
   let mut copy = match *token {
      TokenTree::Group(ref group) => {
         TokenTree::Group(Group::new(
            group.delimiter(),
            group
               .stream()
               .into_iter()
               .map(|inner| expansion_token(&inner))
               .collect(),
         ))
      },
      TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => token.clone(),
   };
   copy.set_span(Span::call_site().located_at(token.span()));
   copy
}

/// Writes `Self { member: binding, .. }` or `Self::Variant { .. }`, which
/// matches every struct shape, unit and tuple included.
fn pattern<'item>(
   out: &mut Out,
   variant: Option<&Ident>,
   bindings: impl IntoIterator<Item = (&'item Field, &'item Ident)>,
   rest: bool,
) {
   out.code("Self");
   if let Some(name) = variant {
      out.code("::");
      out.ident(name);
   }

   let mut members = Out::default();
   for (field, binding) in bindings {
      members.tokens([&field.member]);
      members.punct(':');
      members.ident(binding);
      members.punct(',');
   }
   if rest {
      members.code("..");
   }
   out.group(Delimiter::Brace, members);
}

/// Writes the expression that formats one case, returning whether it calls the
/// `__flub_display` helper.
fn display_body<'item>(
   out: &mut Out,
   display: &Display,
   fields: &'item Fields,
   bounds: &mut Vec<Bound<'item>>,
) -> Result<bool, Fault> {
   let mut arguments = Out::default();
   let mut displays = false;

   match *display {
      Display::Transparent(span) => {
         let [ref field] = *fields.list.as_slice() else {
            return Err(Fault::new(
               span,
               "#[error(transparent)] requires exactly one field",
            ));
         };

         bounds.push(Bound {
            ty:          &field.ty,
            requirement: Requirement::Format(FormatTrait::Display),
         });
         out.code("::core::fmt::Display::fmt");
         arguments.ident(&field.binding);
         arguments.code(", __flub_formatter");
      },
      Display::Function(ref path) => {
         out.tokens(path);
         for field in &fields.list {
            arguments.ident(&field.binding);
            arguments.punct(',');
         }
         arguments.code("__flub_formatter");
      },
      Display::Format {
         ref literal,
         arguments: ref user_arguments,
      } => {
         let expansion = format::expand(literal, user_arguments.clone(), fields)?;
         displays = expansion.displays;
         bounds.extend(expansion.bounds.into_iter().map(|(field, format)| {
            Bound {
               ty:          &field.ty,
               requirement: Requirement::Format(format),
            }
         }));

         match expansion.plan {
            Plan::Text(text) => {
               out.code("::core::fmt::Formatter::write_str");
               arguments.code("__flub_formatter,");
               arguments.extend([TokenTree::Literal(text)]);
            },
            Plan::Format(tokens) => {
               let mut format_arguments = Out::default();
               format_arguments.extend(tokens);

               out.code("::core::fmt::Formatter::write_fmt");
               arguments.code("__flub_formatter, ::core::format_args!");
               arguments.group(Delimiter::Parenthesis, format_arguments);
            },
         }
      },
   }

   out.group(Delimiter::Parenthesis, arguments);
   Ok(displays)
}

/// Finds the cause of one case, which is the transparent field, the one marked
/// `#[source]` or `#[from]`, or else a field named `source`.
fn source(fields: &Fields, transparent: bool) -> Result<Option<Source<'_>>, Fault> {
   if transparent {
      return Ok(fields.list.first().map(Source::Transparent));
   }

   let mut marked = fields
      .list
      .iter()
      .filter(|field| field.source.is_some() || field.from.is_some());
   let found = match (marked.next(), marked.next()) {
      (Some(_), Some(second)) => {
         let span = second
            .source
            .or(second.from)
            .unwrap_or_else(|| second.member.span());
         return Err(Fault::new(span, "only one field can be the source"));
      },
      (Some(field), None) => Some(field),
      (None, _) => fields.list.iter().find(|field| field.is_named("source")),
   };

   Ok(found.map(Source::Field))
}

/// Writes the `source()` result for one case.
fn source_expression(out: &mut Out, source: Source<'_>, binding: &Ident) {
   match source {
      Source::Field(field) => source_call(out, binding, field),
      Source::Transparent(field) => {
         let mut arguments = Out::default();
         source_call(&mut arguments, binding, field);
         arguments.code(", ::core::error::Error::source");

         out.code("::core::option::Option::and_then");
         out.group(Delimiter::Parenthesis, arguments);
      },
   }
}

/// Writes `binding.__flub_source()`, spanned to the field type so a source that
/// isn't an error gets its compile error there.
fn source_call(out: &mut Out, binding: &Ident, field: &Field) {
   let span = field
      .ty
      .first()
      .map_or_else(Span::call_site, TokenTree::span);

   out.ident(binding);
   out.punct('.');
   out.ident(&Ident::new(
      "__flub_source",
      Span::call_site().located_at(span),
   ));
   out.group(Delimiter::Parenthesis, Out::default());
}

/// Writes `impl From` for a `#[from]` field, capturing a backtrace into any
/// backtrace field.
fn from_impl(out: &mut Out, item: &Item, case: &Case<'_>, field: &Field) -> Result<(), Fault> {
   let Some(span) = field.from else {
      return Ok(());
   };

   if case
      .fields
      .list
      .iter()
      .any(|other| !ptr::eq(other, field) && !other.is_backtrace())
   {
      return Err(Fault::new(
         span,
         "#[from] allows only a backtrace alongside it",
      ));
   }

   let mut members = Out::default();
   for other in &case.fields.list {
      members.tokens([&other.member]);
      if ptr::eq(other, field) {
         members.code(": __flub_source,");
      } else {
         members.code(": ::core::convert::From::from(::std::backtrace::Backtrace::capture()),");
      }
   }

   let mut body = Out::default();
   body.code("Self");
   if let Some(name) = case.variant {
      body.code("::");
      body.ident(name);
   }
   body.group(Delimiter::Brace, members);

   let mut parameter = Out::default();
   parameter.code("__flub_source:");
   parameter.tokens(&field.ty);

   let mut function = Out::default();
   function.code("fn from");
   function.group(Delimiter::Parenthesis, parameter);
   function.code("-> Self");
   function.group(Delimiter::Brace, body);

   let mut trait_path = Out::default();
   trait_path.code("::core::convert::From<");
   trait_path.tokens(&field.ty);
   trait_path.punct('>');

   header(out, item, trait_path, &[], &[], false);
   out.group(Delimiter::Brace, function);
   Ok(())
}
