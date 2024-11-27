{ pkgs ? import (fetchTarball { url = "https://github.com/NixOS/nixpkgs/archive/dc460ec76cbff0e66e269457d7b728432263166c.tar.gz"; }) {} }:
pkgs.mkShell {
  buildInputs = [
    (pkgs.callPackage ./chromedriver.nix {})
  ];
}
