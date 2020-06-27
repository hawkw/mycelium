args@{ ... }:

let
  moz_overlay = import (builtins.fetchTarball
    "https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz");
  pkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
  rustChannel = pkgs.rustChannelOf { rustToolchain = ./rust-toolchain; };
  rustNightly = rustChannel.rust.override {
    extensions = [ "rust-src" "llvm-tools-preview" ];
  };
  cargoToml = (builtins.fromTOML (builtins.readFile ./Cargo.toml));
in (pkgs.makeRustPlatform {
  cargo = rustNightly;
  rustc = rustNightly;
  stdenv = pkgs.stdenvAdapters.makeStaticBinaries;

}).buildRustPackage {
  name = cargoToml.package.name;
  version = cargoToml.package.version;

  cargoSha256 = "1nmamh1ygrx28k8896ffm01slxsahp55lipd1f9d2w2x0qm6sfwq";
  paths = with pkgs;
    [
      zlib
      git
      bash
      direnv
      binutils
      stdenv
      cacert
      gcc
      cmake
      rustup
      pkg-config
      openssl
      rustup
      rust_nightly
      (glibcLocales.override { locales = [ "en_US.UTF-8" ]; })
      gnumake
      autoconf
      qemu
    ] ++ stdenv.lib.optional stdenv.isDarwin [ Security libiconv ];
  passthru = {
    LOCALE_ARCHIVE = "${pkgs.glibcLocales}/lib/locale/locale-archive";
    LC_ALL = "en_US.UTF-8";
    SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    GIT_SSL_CAINFO = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    CURL_CA_BUNDLE = "${pkgs.cacert}/etc/ca-bundle.crt";
  };

  # depsBuildHost = (environment.dependencies.depsBuildHost targetPkgs);
  depsBuildBuild = [ pkgs.buildPackages.zlib ];
  # depsHostTarget = (environment.dependencies.depsHostTarget targetPkgs);
  # depsHostBuild = (environment.dependencies.depsHostBuild targetPkgs);
  # nativeBuildInputs = (environment.dependencies.nativeBuildInputs hostPkgs);
}
