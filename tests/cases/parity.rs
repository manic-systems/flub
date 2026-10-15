use core::{
   error::Error as StdError,
   fmt::{
      self,
      Debug,
      Display,
      Formatter,
   },
   ops::Deref,
};
use std::{
   io,
   path::PathBuf,
};

/// A report like `anyhow::Error` that only derefs to `dyn Error`.
#[derive(Debug)]
pub struct Report(pub Box<dyn StdError + Send + Sync>);

impl Deref for Report {
   type Target = dyn StdError + Send + Sync;

   fn deref(&self) -> &Self::Target {
      &*self.0
   }
}

impl Display for Report {
   fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
      Display::fmt(&self.0, f)
   }
}

/// A user type that shares its name with `std::path::Path`.
#[derive(Debug)]
pub struct Path;

impl Display for Path {
   fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
      f.write_str("mine")
   }
}

#[derive(Error, Debug)]
#[error("unit struct")]
pub struct Unit;

#[derive(Error, Debug)]
#[error("{0}")]
pub struct Delegated(pub String);

#[derive(Error, Debug)]
#[error("{0:?}")]
pub struct DebugOnly(pub Vec<u8>);

#[derive(Error, Debug)]
#[rustfmt::skip]
#[error("escaped \u{7b}code\u{7d} \x41\
         continued \t{code:?}")]
pub struct Escaped {
   pub code: u8,
}

#[derive(Error, Debug)]
pub enum Custom {
   #[error(fmt = render)]
   Pair(Vec<u8>, String),
   #[error(fmt = render_named)]
   Named { left: String },
}

fn render(left: &[u8], right: &str, formatter: &mut Formatter<'_>) -> fmt::Result {
   write!(formatter, "custom {}/{right}", left.len())
}

fn render_named(left: &str, formatter: &mut Formatter<'_>) -> fmt::Result {
   write!(formatter, "named {left}")
}

#[derive(Error, Debug)]
#[error("{{literal}} braces \u{e9} and \"quotes\"\n")]
pub struct Braces;

#[derive(Error, Debug)]
#[error(r#"raw {name} "q" {{x}}"#)]
pub struct Raw {
   pub name: &'static str,
}

#[derive(Error, Debug)]
pub enum Kinds {
   #[error("io failed for {path}")]
   Io {
      path:  PathBuf,
      #[source]
      cause: io::Error,
   },
   #[error("path {0:?} vs {0}")]
   Path(PathBuf),
   #[error("user path {0}")]
   UserPath(Path),
   #[error("hex {0:#x} {0:>6} {1:.2} {1:e}")]
   Numbers(u32, f64),
   #[error("len {} of {name}, {extra}", .items.len(), extra = .items.first().copied().unwrap_or(0))]
   Args { name: String, items: Vec<u8> },
   #[error("first {}", first(.0))]
   Call(Vec<u8>),
   #[error("{}", if .flag == &true { "on" } else { "off" })]
   Flag { flag: bool },
   #[error(transparent)]
   Transparent(#[from] Leaf),
   #[error("boxed")]
   Boxed(#[source] Box<dyn StdError + Send + Sync>),
   #[error("optional boxed")]
   OptionalBoxed(#[source] Option<Box<dyn StdError + Send + Sync>>),
   #[error("named source {source}")]
   NamedSource { source: Leaf },
   #[error("optional")]
   Optional {
      #[source]
      inner: Option<Leaf>,
   },
   #[error(transparent)]
   Report(Report),
   #[error("no source {type}")]
   RawField { r#type: u8 },
}

fn first(items: &[u8]) -> u8 {
   items.first().copied().unwrap_or(0)
}

#[derive(Error, Debug)]
#[error("leaf {0}")]
pub struct Leaf(pub u8);

#[derive(Error, Debug)]
pub enum Generic<T, E>
where
   T: Debug,
{
   #[error("value {0}")]
   Value(T),
   #[error("wrapped")]
   Wrapped(#[source] E),
   #[error("{0:?} and {1}")]
   Both(Vec<T>, E),
}

#[derive(Error, Debug)]
#[error("lifetime {text}")]
pub struct Borrowed<'text> {
   pub text: &'text str,
}

#[derive(Error, Debug)]
#[error("attributed")]
pub struct Attributed<#[expect(unused, reason = "an attribute on a generic parameter")] T>(
   pub T,
);

#[derive(Error, Debug)]
#[error("const {0:?}")]
pub struct Array<const N: usize = { 2 + 1 }>(pub [u8; N]);

macro_rules! coded {
   ($vis:vis $name:ident, $message:literal, $field:ident: $ty:ty) => {
      #[derive(Error, Debug)]
      #[error($message)]
      $vis struct $name {
         $vis $field: $ty,
      }
   };
}

coded!(pub FromMacro, "macro {code}", code: u8);

#[derive(Error, Debug)]
#[error("transparent box")]
pub struct BoxedTransparentOuter(#[from] pub Box<dyn StdError + Send + Sync>);

#[derive(Error, Debug)]
#[error(transparent)]
pub struct TransparentStruct(#[from] pub io::Error);

#[inline]
pub fn values() -> Vec<Box<dyn StdError>> {
   vec![
      Box::new(Unit),
      Box::new(Escaped { code: 5 }),
      Box::new(Custom::Pair(vec![1], "two".into())),
      Box::new(Custom::Named { left: "three".into() }),
      Box::new(Delegated("hi".into())),
      Box::new(DebugOnly(vec![1, 2])),
      Box::new(Braces),
      Box::new(Raw { name: "n" }),
      Box::new(Kinds::Io {
         path:  "/a/b".into(),
         cause: io::Error::other("disk"),
      }),
      Box::new(Kinds::Path("/p".into())),
      Box::new(Kinds::UserPath(Path)),
      Box::new(Kinds::Numbers(255, 3.25)),
      Box::new(Kinds::Args {
         name:  "n".into(),
         items: vec![7, 8],
      }),
      Box::new(Kinds::Call(vec![9])),
      Box::new(Kinds::Flag { flag: true }),
      Box::new(Kinds::from(Leaf(1))),
      Box::new(Kinds::Boxed(Box::new(Leaf(2)))),
      Box::new(Kinds::OptionalBoxed(Some(Box::new(Leaf(3))))),
      Box::new(Kinds::NamedSource { source: Leaf(3) }),
      Box::new(Kinds::Optional {
         inner: Some(Leaf(4)),
      }),
      Box::new(Kinds::Optional { inner: None }),
      Box::new(Kinds::Report(Report(Box::new(Leaf(5))))),
      Box::new(Kinds::RawField { r#type: 1 }),
      Box::new(Generic::<u8, Leaf>::Value(6)),
      Box::new(Generic::<u8, Leaf>::Wrapped(Leaf(7))),
      Box::new(Generic::<u8, Leaf>::Both(vec![1], Leaf(8))),
      Box::new(Delegated(Borrowed { text: "t" }.to_string())),
      Box::new(Attributed(4_u8)),
      Box::new(Array([1, 2, 3])),
      Box::new(FromMacro { code: 2 }),
      Box::new(BoxedTransparentOuter::from(
         Box::<dyn StdError + Send + Sync>::from(Leaf(9)),
      )),
      Box::new(TransparentStruct::from(io::Error::other(Leaf(10)))),
   ]
}
