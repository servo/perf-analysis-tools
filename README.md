Perf analysis tools
===================

## How to run a multi-sample study

Our tooling has first-class support for running benchmarks across a matrix of CPU configs, sites, and engines, then collating their results into reports like [our November 2024 report](https://github.com/servo/servo/wiki/Servo-Benchmarking-Report-(November-2024)).

Results are organised by CPU config, then by site, then by engine. The results for one combination of (CPU configuration, site, engine) is known as a **sample**. For example, results for the sample with CPU config key “2cpu”, site key “servo.org”, and engine key “servo.old” would be found at ./2cpu/servo.org/servo.old.

1. Create a study.
   ```sh
   $ mkdir studies/foo
   $ cp studies/example/study.toml studies/foo
   ```

2. Edit study.toml to define CPU configs, sites, engines, and other settings.
   ```sh
   $ $EDITOR studies/foo/study.toml
   ```

3. Collect results. For Servo samples, this creates `trace*.html`, `servo*.pftrace`, and `manifest*.json`. For Chromium samples, this creates `chrome*.pftrace`.
   ```sh
   $ cargo run -r -- collect studies/foo
   ```
   If collection for a sample fails, the program will fail loudly with a non-zero exit status. In this case, run the `collect` command again, and collection will restart from the sample that failed.

4. Analyse results. This creates `summaries.txt` and `summaries.json`. For Chromium samples, this also creates `chrome*.json`, which are `chrome*.pftrace` [converted to JSON](https://perfetto.dev/docs/quickstart/traceconv).
   ```sh
   $ cargo run -r -- analyse studies/foo
   ```
   Our analysis code is currently written to consume the old Chrome JSON trace format, but we should migrate it to consume Perfetto traces directly, because that will simplify and speed up analysis.

5. Generate the report.
   ```sh
   $ cargo run -r -- report studies/foo
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

## Working with single samples

This is a simpler workflow, ideal for situations where you only have one CPU configuration, one site, and very few engines.

### How to create a single sample for Servo or Chromium

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

### How to analyse a single Servo sample

Both trace formats are required for now, because some metrics like TimeToFirstPaint and TimeToFirstContentfulPaint are only in the HTML traces, while some events like ScriptEvaluate are only in the Perfetto traces.

Use the `servo` command, where `<url>` is the same URL as the page you loaded:

```
$ RUST_LOG=analyse=info cargo run -r servo <url> <manifest.json> [manifest.json ...]
```

`--print-pwm` tells you in the terminal running Servo when you’ve waited long enough for the Time To Interactive metric to appear in your trace.

### How to analyse a single Chromium sample

Use the `chromium` command, where `<url>` is the same URL as the page you loaded:

```
$ python traceconv json chrome.pftrace chrome.json
$ RUST_LOG=analyse=info cargo run -r chromium <url> <chrome.json> [chrome.json ...]
```

### How to generate a combined [Chrome JSON trace](https://docs.google.com/document/d/1CvAClvFfyA5R-PhYUmn5OOQtYMH4h6I0nSsKchNAySU) for a set of related samples

Use the `combined` command, where each `<command>` is a `servo` or `chromium` command from above:

```
$ RUST_LOG=analyse=info cargo run -r combined <command> [[-- <command>] ...]
```

These traces can be opened in the [Perfetto UI](https://ui.perfetto.dev).
