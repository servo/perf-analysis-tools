# Changelog

## 0.2.0

- Add support for Servo traces from the `tracing-perfetto-file` backend, including interned event
  and debug annotation names, packet defaults, and delta-encoded timestamps.
- **Breaking:** Servo traces from the older `tracing-perfetto` backend are no longer supported.
  Use release 0.1.0 for those traces.

## 0.1.0

- Initial release.
- Supports Servo traces from the `tracing-perfetto` backend and Chromium JSON traces.
