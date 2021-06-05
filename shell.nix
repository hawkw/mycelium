scope@{ pkgs ? import <nixpkgs> { } }:

let env = (import ./default.nix scope);

in pkgs.mkShell {
  # also install qemu into the dev shell, for testing
  packages = [ pkgs.qemu_full ];

  LOCALE_ARCHIVE = "${pkgs.glibcLocales}/lib/locale/locale-archive";
  LC_ALL = "en_US.UTF-8";
  OPENSSL_DIR = "${pkgs.openssl.dev}";
  OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
  SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  GIT_SSL_CAINFO = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  CURL_CA_BUNDLE = "${pkgs.cacert}/etc/ca-bundle.crt";
  CARGO_TERM_COLOR = "always";
  RUST_BACKTRACE = "1";
  buildInputs = [ (import ./default.nix { inherit pkgs; }) ];
}
