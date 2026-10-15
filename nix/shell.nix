{
  mkShell,
  rustc,
  cargo,
  rust-analyzer,
  rustfmt,
  clippy,
  rustPlatform,
  extraPackages ? [ ],
}:
mkShell {
  name = "flub";

  strictDeps = true;

  nativeBuildInputs = [
    rustc
    cargo
    rust-analyzer
    rustfmt
    clippy
  ]
  ++ extraPackages;

  env.RUST_SRC_PATH = "${rustPlatform.rustLibSrc}";
}
