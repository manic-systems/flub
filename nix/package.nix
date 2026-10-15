{
  lib,
  rustPlatform,
}:
let
  cargoTOML = (lib.importTOML ../Cargo.toml).workspace.package;
in
rustPlatform.buildRustPackage {
  pname = "flub";
  inherit (cargoTOML) version;

  src =
    let
      fs = lib.fileset;
      s = ../.;
    in
    fs.toSource {
      root = s;
      fileset = fs.unions [
        (s + /flub-derive)
        (s + /src)
        (s + /tests)
        (s + /Cargo.lock)
        (s + /Cargo.toml)
      ];
    };

  cargoLock.lockFile = ../Cargo.lock;

  cargoTestFlags = [ "--workspace" ];

  strictDeps = true;
  enableParallelBuilding = true;

  meta = {
    description = "A zero-dependency derive(Error)";
    license = lib.licenses.eupl12;
    maintainers = with lib.maintainers; [ amaanq ];
  };
}
