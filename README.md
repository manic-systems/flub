# flub

A zero-dependency `#[derive(Error)]` that writes the `Error`, `Display`, and
`From` impls for your error types.

```rust
use flub::Error;

#[derive(Error, Debug)]
pub enum LoadError {
   #[error("reading {path}")]
   Read { path: std::path::PathBuf, #[source] cause: std::io::Error },
   #[error("bad header {0:#x}")]
   Header(u32),
   #[error("{} entries, expected {expected}", .found.len())]
   Count { found: Vec<u8>, expected: usize },
   #[error(transparent)]
   Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}
```

## Attributes

- `#[error("...", args...)]` on a struct, a variant, or an enum as the fallback
  for its variants. Fields are named in the format string as `{field}` or `{0}`,
  and in the arguments as `.field` or `.0`. A bare `{path}` on a `Path` or
  `PathBuf` field formats through `Path::display`.
- `#[error(fmt = path)]` calls `path(&field, ..., formatter)` with every field
  in order.
- `#[error(transparent)]` forwards `Display` and `source` to the only field.
- `#[source]` marks the cause. A field named `source` is the cause without it.
  The cause may be `Option<T>`, `Box<dyn Error>`, or a type that derefs to
  `dyn Error`, such as `anyhow::Error` or `eyre::Report`.
- `#[from]` also generates `From`. The only other field it allows is a
  `Backtrace` or `Option<Backtrace>`, which is filled by `Backtrace::capture`.

Without any `#[error(...)]`, only `Error` is derived and `Display` is left to
the user. Generic fields get the bounds their placeholders need.

## Features

- `std`, on by default, formats a bare `{path}` through `Path::display`. Without
  it, everything but `Backtrace` fields needs only `core`.
- `provide` generates `Error::provide` on nightly, which needs
  `#![feature(error_generic_member_access)]` in the deriving crate. It forwards
  to the source first, so the deepest backtrace wins, then offers the type's own
  `Backtrace` field.

## License

[EUPL-1.2](LICENSE)
