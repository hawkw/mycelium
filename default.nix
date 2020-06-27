args@{ ... }:

let
  moz_overlay = import (builtins.fetchTarball
    "https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz");
  pkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
  rust_channel = pkgs.rustChannelOf { rustToolchain = ./rust-toolchain; };
  rust_nightly = rust_channel.rust.override {
    extensions = [ "rust-src" "llvm-tools-preview" ];
  };
in pkgs.buildEnv {
  name = "mycelium-env";
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
}
