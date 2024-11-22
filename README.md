Perf analysis tools
===================

## How to run Servo or Chromium

Make sure you build Servo with `--features tracing-perfetto`. When benchmarking the root of a site, be sure to include the trailing slash any time a URL is needed.

Run benchmarks as follows:

```
$ ./benchmark-servo.sh ~/path/to/servoshell http://example.com/ 30 ./example.com.servo
$ ./benchmark-chromium.sh google-chrome-stable http://example.com/ 30 ./example.com.chromium
```

We also recommend configuring your window manager to move the windows to a secondary monitor, where they can be kept visible but not focused. For example, in [i3](https://i3wm.org/docs/userguide.html), where `7` is a workspace that has windows but is not visible on any monitor:

```
$ cat ~/.config/i3/config
for_window [instance="^google-chrome [(]" class="^Google-chrome$"] floating enable
for_window [instance="^servo$" class="^servo$"] floating enable
assign [instance="^google-chrome [(]" class="^Google-chrome$"] 7
assign [instance="^servo$" class="^servo$"] 7
```

## How to replay page loads without relying on network traffic (Linux only)

Create a `mitmproxy` group and add it to your user’s supplementary groups:

```
$ sudo groupadd mitmproxy
$ sudo usermod -aG mitmproxy $(whoami)
```

Or if you are on NixOS:

```
users.groups.mitmproxy = {
  members = [ "<your username>" ];
};
```

Then record a dump and replay it, following the prompts:

```
$ sudo ./start-mitmproxy.sh record path/to/example.com.dump
$ sudo ./start-mitmproxy.sh replay path/to/example.com.dump
```

## How to analyse Servo’s HTML and Perfetto traces

Both trace formats are required for now, because some metrics like TimeToFirstPaint and TimeToFirstContentfulPaint are only in the HTML traces, while some events like ScriptEvaluate are only in the Perfetto traces.

Use the `servo` command, where `<url>` is the same URL as the page you loaded:

```
$ RUST_LOG=analyse=info cargo run -r servo <url> <trace.html> [trace.html ...]
```

`--print-pwm` tells you in the terminal running Servo when you’ve waited long enough for the Time To Interactive metric to appear in your trace.

## How to analyse Chromium Perfetto traces (`--trace-startup --trace-startup-file`)

Use the `chromium` command, where `<url>` is the same URL as the page you loaded:

```
$ python traceconv json chrome.pftrace chrome.json
$ RUST_LOG=analyse=info cargo run -r chromium <url> <chrome.json> [chrome.json ...]
```

## How to generate a combined [Chrome JSON trace](https://docs.google.com/document/d/1CvAClvFfyA5R-PhYUmn5OOQtYMH4h6I0nSsKchNAySU)

Use the `combined` command, where each `<command>` is a `servo` or `chromium` command from above:

```
$ RUST_LOG=analyse=info cargo run -r combined <command> [[-- <command>] ...]
```

These traces can be opened in the [Perfetto UI](https://ui.perfetto.dev).
