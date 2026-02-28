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
  version = "139.0.7258.154";

  src = let
    json = builtins.fromJSON (builtins.readFile (fetchurl {
      url = "https://googlechromelabs.github.io/chrome-for-testing/${version}.json";
      hash = "sha256-FO5csnDLVKTRj+YmkWErqzRTTZtnRoXw8CmmiWtXR5k=";
    }));
    url = (lib.findFirst (d: d.platform == "linux64") null json.downloads.chromedriver).url;
  in fetchurl {
    url = url;
    hash = "sha256-C445JMIIfL6U57rSfnCEq5CK61NEGk5afx8BwLgqh14=";
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
