{ lib
, stdenv
, fetchurl
, unzip
, autoPatchelfHook
, glib
, nss
, xorg
}:

stdenv.mkDerivation rec {
  pname = "chromedriver";
  version = "138.0.7204.168";

  src = let
    json = builtins.fromJSON (builtins.readFile (fetchurl {
      url = "https://googlechromelabs.github.io/chrome-for-testing/${version}.json";
      hash = "sha256-47Ig+EDxFijHLEoMHWFbAWb1xdGliRedBy0IuL+POjo=";
    }));
    url = (lib.findFirst (d: d.platform == "linux64") null json.downloads.chromedriver).url;
  in fetchurl {
    url = url;
    hash = "sha256-PjU55ZNAfc/VYl+PA4Tvyr0MNHDU1ZDBnWoDOWlGO5A=";
  };

  nativeBuildInputs = [
    unzip
    autoPatchelfHook
  ];

  buildInputs = [
    glib
    nss
    xorg.libxcb
  ];

  sourceRoot = ".";

  installPhase = ''
    install -m755 -D chromedriver-linux64/chromedriver $out/bin/chromedriver
  '';
}
