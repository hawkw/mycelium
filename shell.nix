scope@{ pkgs ? import <nixpkgs> { } }:

let env = (import ./default.nix scope);

in pkgs.mkShell {
  # also install qemu into the dev shell, for testing
  packages = with pkgs; [ qemu gdb direnv just cargo-nextest ]
    ++ env.buildInputs
    ++ env.nativeBuildInputs;

  buildInputs = [ (import ./default.nix { inherit pkgs; }) ];

  LOCALE_ARCHIVE = "${pkgs.glibcLocales}/lib/locale/locale-archive";
  LC_ALL = "en_US.UTF-8";
  OPENSSL_DIR = "${pkgs.openssl.dev}";
  OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
  SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  GIT_SSL_CAINFO = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  CURL_CA_BUNDLE = "${pkgs.cacert}/etc/ca-bundle.crt";
  CARGO_TERM_COLOR = "always";
  RUST_BACKTRACE = "1";
  # Somehow this is necessary to make cargo builds not die when running the
  # buildscript for `libz-sys`. I don't know why this fixes it.
  CARGO_PROFILE_DEV_BUILD_OVERRIDE_DEBUG = "true";
}
