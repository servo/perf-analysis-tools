Perf analysis tools
===================

## How to run Servo or Chromium

```
$ ./benchmark-servo.sh ~/path/to/servoshell http://example.com 30 ./example.com.servo
$ ./benchmark-chromium.sh google-chrome-stable http://example.com 30 ./example.com.chromium
```

We also recommend configuring your window manager to move the windows to a secondary monitor, where they can be kept visible but not focused. For example, in i3:

```
$ cat ~/.config/i3/config
for_window [instance="^google-chrome [(]" class="^Google-chrome$"] floating enable
for_window [instance="^servo$" class="^servo$"] floating enable
assign [instance="^google-chrome [(]" class="^Google-chrome$"] 9
assign [instance="^servo$" class="^servo$"] 9

$ cat custom-servo-window-commands.sh
xdotool search --sync --onlyvisible --pid $1 --class servo windowmove $((2560+2)) $((0+28))

$ cat custom-chromium-window-commands.sh
xdotool search --sync --onlyvisible --pid $1 --class google-chrome windowmove $((2560+2)) $((0+28))
i3-msg 'workspace back_and_forth'
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
