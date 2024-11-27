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
  version = "130.0.6723.91";

  src = let
    json = builtins.fromJSON (builtins.readFile (fetchurl {
      url = "https://googlechromelabs.github.io/chrome-for-testing/${version}.json";
      hash = "sha256-LS6n73mzL46AQtv7FvCRguGf090NyaPvotKxUueOIj0=";
    }));
    url = (lib.findFirst (d: d.platform == "linux64") null json.downloads.chromedriver).url;
  in fetchurl {
    url = url;
    hash = "sha256-qMlM6ilsIqm8G5KLE4uGVb/s2bNyZSyQmxsq+EHKX/c=";
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
