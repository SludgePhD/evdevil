# Changelog

## v0.1.3

- Add `EventReader::next_report` for fetching whole `Report`s from the device rather than events.

## v0.1.2

- Add `serde` feature that implements `Serialize` and `Deserialize` for some of the event code
  wrapper types like `Key`, `Rel`, `Abs`, etc.

## v0.1.1

- Renamed `Evdev::can_read` and `UinputDevice::can_read` to `Evdev::is_readable`
  and `UinputDevice::is_readable`, respectively (with `can_read` becoming a
  deprecated alias).
- Added `Evdev::block_until_readable` and `UinputDevice::block_until_readable`.

## v0.1.0

Initial release.
