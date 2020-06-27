args@{ pkgs ? import <nixpkgs> { } }:

let default = (import ./default.nix { });
in pkgs.mkShell {
  CARGO_TERM_COLOR = "always";
  RUST_BACKTRACE = "1";
  buildInputs = default.buildInputs;
}
