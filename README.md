# w4on2

Tools to assist in composing music for WASM-4.

Though there are still [unfinished parts](#todos-and-known-bugs), it is very much usable and a huge upgrade from the old project (called `w4on`.)

# Parts

- runtime: C library used in WASM-4 projects to play back w4on2 songs.
- cli: Tool to either `convert` MIDI & TOML to a w4on2 file, or to `bounce` a w4on2 file into a wav export.
- plugin: A VST3/CLAP audio plugin to assist in composing new songs.
- shared: Rust code shared between plugin and cli.

# Usage

w4on2 assumes you already have a digital audio workstation capable of loading VST3 or CLAP plugins, and with the ability to export MIDI files from projects. This is why `w4on2_plugin` was made.
If you do not have a DAW, w4on2 overall might not be the right fit for you, but you can still use `w4on2_cli` to manually convert MIDI files together with a w4on2 config TOML file.

One important thing to keep in mind is to only ever have one instance of the w4on2 plugin in a project to remain accurate with the WASM-4 APU.

## Showcase/tutorial

[![w4on2 showcase/tutorial](https://i.imgur.com/kZppK6T.png)](https://www.youtube.com/watch?v=4VezbcXImcE)

## Installation

w4on2 has mostly been tested on Windows, but it should work on other platforms too.
Please submit a PR if you get something working!

### Windows 

Download: For now, [download a build from Github Actions](https://github.com/JerwuQu/w4on2/actions?query=branch%3Amaster+is%3Asuccess).

If using VST3, put into `C:\Program Files\Common Files\VST3`.

If using CLAP, put into `C:\Program Files\Common Files\CLAP`.

## FL Studio

Specifically for FL Studio, one trick you can do for simple MIDI exports and avoid using the destructive "Prepare for MIDI export" macro, is create a a MIDI Out channel with "Map note color to MIDI channel" and route that to the w4on2 plugin instance on the same MIDI port.
This makes it easy to compose and export from the same project without having to tweak any project settings.

# Build

`cargo build --release`

# Architecture

## Terminology

- **Event**: Describes an action or changing configuration. Are used from everything to setting instrument parameters to playing notes. In format: Variable-size.
- **Track**: Linked to one MIDI channel, describing one data-stream in the format. For simplicity, each Track has one Instrument in the editor.
- **Pattern**: A list of Events. Shared between all Tracks.
- **Instrument**: Abstract set of parameters not actually present in the w4on2 format, but are instead set via Events.

## General outline

The player in `runtime` consumes a w4on2 data buffer and calls into a WASM-4 compatible `tone` function (user-supplied for flexibility).

`runtime`, rather than calling `tone` only once per note, continously calls `tone` and implements envelopes and effects on its own.
This is required to be able to work with NoteOn-NoteOff type Events, which is required for live playback.

`runtime` has two parts, the actual w4on2 runtime, and the w4on2 player.
The player is the high-level interface used for playing back music and sounds, while the runtime handles parsing Events as they come in.

`runtime` takes a custom `tone` function rather than import the WASM-4 one to make it easier to do opererations before actually executing the tone, such as temporarily interrupting music for SFX.
It is also needed for the live playback feature.

`shared` compiles `runtime` into itself to be able to accurately simulate real playback in the tools.

`wasm4_apu` is a ported version of the real WASM-4 native APU but with removed global state and support for different sample-rates rather than being locked to 44100 Hz.

With all this, we can use the same underlying event logic for all playback.

`SongConfig` contains somewhat user-friendly parameters serializable to TOML.

`TrackEvent`s are the different events consumed by the w4on2 runtime, but can still use high-level types until they are serialized into binary data following `W4ON2_FMT_*` definitions in `w4on2.h`.

`MidiEventMapper` takes `SongTrackConfig`s from the `SongConfig` and, using stateful logic, maps MIDI events to `TrackEvent`s.

## MIDI layout

Regular MIDI notes on MIDI channels. Only the most essential MIDI messages are handled, and the rest is left up to the instrument configuration.

## w4on2 format

**The binary format for w4on2 is *not* stable.**

This means w4on2 files exported with one version will likely not work with the runtime of another.
There is also the possibilty of effect parameters being changed or extended which could affect the sound of existing songs.

In practice, this is more of a benefit than a downside because it leaves the possibility for things like adding new effects or improving the compression,
and any project using w4on2 can simply stay on the old version or re-convert their songs again.

### Overview

One track per channel.

Basically do same as w4on but with patterns (one layer of indirection).

### Binary

Counts are laid out in the header so we can do O(1) lookups for the start of both patterns and tracks by their index.

Sizes for patterns and tracks are not provided. They are instead implied to end where the next data begins.

#### w4on2 file

```
- Header -
(file_size:u16)
(pattern_count:u8)
(track_count:u8)
(pattern_offsets:[u16...])
(track_offsets:[u8...])
- Data -
(pattern_events:[[Event...]...])
(track_patterns:[[u8...]...])
```

#### Event

See `w4on2.h` FMT or `protospan.js`.

## TOML layout

A list of instruments per MIDI channel. The idea is not to write TOML directly but instead use the audio plugin to generate it, though it can still be used manually.
It is especially useful as a preset for new projects, or as backup if the plugin receives a breaking update.

### Example
```toml
[[channels]]
channel = {"pulse1" = "12.5%"}
adsr = [1, 1, 100, 1]

[[channels]]
channel = "triangle"

[[channels]]
channel = "noise"
```

# TODOs and known bugs

- Delay
- Friendly names for channels in plugin/TOML: `nickname` field exists but need some way to do popups since baseview-egui text input is b0rk.
- `bounce` function in plugin.
- Cruncher currently gives different sizes on each invocation. Something is not fully efficient. Look into? Not hugely important.
- Sane logging. Currently no way to disable `nih_log`. Fix in `nih_log` fork?
- Ability to automate parameters via MIDI CC messages.
- There could be a recording feature in the plugin for even simpler export, though it would require allocations in `process` and would overall be finicky to use.
- Make sure terminology is sane so it's easy to understand what everything means.
- Refactor w4on2.c to use floats and see if it improves or worsens size. It surely improves readability in some places.
- Configurable update-rate (aka. tick alignment interval) for devices/runtimes that don't call `tone` at 60 Hz.

# License

Everything in this repository is licensed as GPLv3 with the exception of files in the `runtime` directory which are also available under MIT.
