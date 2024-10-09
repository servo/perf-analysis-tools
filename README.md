Perf analysis tools
===================

## How to analyse Servo HTML traces (`--profiler-trace-path`)

Use the `servo` command, where `<url>` is the same URL as the page you loaded:

```
$ path/to/servo --profiler-trace-path=trace.html --print-pwm <url>
$ RUST_LOG=analyse=info cargo run -r servo <url> <trace.html> [trace.html ...]
```

`--print-pwm` tells you in the terminal running Servo when you’ve waited long enough for the Time To Interactive metric to appear in your trace.

## How to analyse Chromium Perfetto traces (`--trace-startup --trace-startup-file`)

Use the `chromium` command, where `<url>` is the same URL as the page you loaded:

```
$ google-chrome-stable --trace-startup --trace-startup-file=chrome.pftrace <url>
$ python traceconv json chrome.pftrace chrome.json
$ RUST_LOG=analyse=info cargo run -r chromium <url> <chrome.json> [chrome.json ...]
```

## How to generate a combined [Chrome JSON trace](https://docs.google.com/document/d/1CvAClvFfyA5R-PhYUmn5OOQtYMH4h6I0nSsKchNAySU)

Use the `combined` command, where each `<command>` is a `servo` or `chromium` command from above:

```
$ RUST_LOG=analyse=info cargo run -r combined <command> [[-- <command>] ...]
```

These traces can be opened in the [Perfetto UI](https://ui.perfetto.dev).
