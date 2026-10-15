//! Support traits for the derived impls. Not public API.

use core::{
   error::Error,
   fmt::Display,
   ops::Deref,
};
#[cfg(feature = "std")]
use std::path::{
   self,
   Path,
   PathBuf,
};

/// Views a `#[source]` field as `dyn Error`, whether the field is a sized
/// error or already a trait object.
pub trait Source {
   /// The field as the cause.
   fn __flub_source(&self) -> Option<&(dyn Error + 'static)>;
}

impl<T: Error + 'static> Source for T {
   #[inline]
   fn __flub_source(&self) -> Option<&(dyn Error + 'static)> {
      Some(self)
   }
}

impl Source for dyn Error + 'static {
   #[inline]
   fn __flub_source(&self) -> Option<&(dyn Error + 'static)> {
      Some(self)
   }
}

impl Source for dyn Error + Send + 'static {
   #[inline]
   fn __flub_source(&self) -> Option<&(dyn Error + 'static)> {
      Some(self)
   }
}

impl Source for dyn Error + Send + Sync + 'static {
   #[inline]
   fn __flub_source(&self) -> Option<&(dyn Error + 'static)> {
      Some(self)
   }
}

/// An [`Option`] cause, picked by method resolution before [`PointerSource`].
pub trait OptionSource {
   /// The cause when present.
   fn __flub_source(&self) -> Option<&(dyn Error + 'static)>;
}

impl<T: Source> OptionSource for Option<T> {
   #[inline]
   fn __flub_source(&self) -> Option<&(dyn Error + 'static)> {
      self.as_ref().and_then(Source::__flub_source)
   }
}

/// An [`Option`] of a report type that only derefs to `dyn Error`, reached
/// through one more autoref. The cause borrows from
/// the field rather than from that autoref.
pub trait PointerSource<'field> {
   /// The dereferenced cause when present.
   fn __flub_source(&self) -> Option<&'field (dyn Error + 'static)>;
}

impl<'field, T> PointerSource<'field> for &'field Option<T>
where
   T: Deref,
   T::Target: Source,
{
   #[inline]
   fn __flub_source(&self) -> Option<&'field (dyn Error + 'static)> {
      self.as_deref().and_then(Source::__flub_source)
   }
}

/// Displays a bare `{field}`, sending a [`Path`] or [`PathBuf`] through
/// [`Path::display`].
///
/// The impls key on the real type, so aliases work and a user type named
/// `Path` is untouched.
pub trait AsDisplay<'item> {
   /// What the placeholder formats.
   type Target: Display;

   /// The field as its displayed form.
   fn __flub_display(&'item self) -> Self::Target;
}

impl<'item, T: Display + ?Sized + 'item> AsDisplay<'item> for &T {
   type Target = &'item T;

   #[inline]
   fn __flub_display(&'item self) -> &'item T {
      self
   }
}

#[cfg(feature = "std")]
impl<'item> AsDisplay<'item> for Path {
   type Target = path::Display<'item>;

   #[inline]
   fn __flub_display(&'item self) -> path::Display<'item> {
      self.display()
   }
}

#[cfg(feature = "std")]
impl<'item> AsDisplay<'item> for PathBuf {
   type Target = path::Display<'item>;

   #[inline]
   fn __flub_display(&'item self) -> path::Display<'item> {
      self.display()
   }
}
