# Equalizer Design

A system-wide 10-band equalizer, set from a new **Equalizer** page in Settings, with built-in presets. The page supports both looks (glass and pixel).

## Engine

Rune runs a PipeWire filter-chain as a child process: `pipewire -c $XDG_RUNTIME_DIR/rune-shell/equalizer.conf`. Nothing new is linked; `pipewire`, `pw-cli`, `pw-dump` and `pactl` are already installed with PipeWire.

- The chain creates a sink named `rune_equalizer` ("Rune Equalizer"). Its playback side targets the output the user picked (`target.object`).
- Graph: `linear` preamp, then `bq_lowshelf` at 31 Hz, `bq_peaking` at 62, 125, 250, 500, 1k, 2k, 4k and 8k Hz, and `bq_highshelf` at 16 kHz, in series. Q is 1.0 on the peaking bands and 0.7 on the shelves.
- **Preamp:** `Mult = 10^(-max(0, max band gain) / 20)`, so boosting never clips. It is set automatically, not by the user.
- **Live edits:** `pw-cli set-param <node id> Props '{ params = [ "b3:Gain" 4.0 … ] }'`. No restart, no audible gap.
- **Turning it on:** remember the current default sink, start the child, wait (bounded) for the node to appear, then `pactl set-default-sink rune_equalizer`.
- **Turning it off, quitting, or crashing:** restore the remembered default sink, then kill the child and wait for it to exit. `rune-shell quit` still returns only after this is done.
- **Picking an output while the EQ is on:** the Sound page's "set default output" changes the chain's `target.object` and the remembered sink, not the default sink.

Verified on this machine (PipeWire 1.6.11) on 2026-11-18: the child creates the sink, `set-param` changes `b1:Gain` from 0 to 6.0 live, and killing the child removes the node.

## Data

```toml
[equalizer]
enabled = false
bands = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]  # dB, clamped to ±12
```

Validation: `bands` must have exactly 10 finite numbers, otherwise parsing fails with a named error. Values are clamped to −12…+12 dB. The preset is not stored: it is whichever preset matches `bands`, otherwise "Custom". Band edits snap to 0.5 dB, so dragging back to a preset's values selects it again.

Presets (dB, 31 Hz → 16 kHz):

| id | Label | Bands |
|---|---|---|
| flat | Flat | 0 0 0 0 0 0 0 0 0 0 |
| bass | Bass Boost | 6 5 4 2 0 0 0 0 0 0 |
| treble | Treble Boost | 0 0 0 0 0 1 2 4 5 6 |
| vocal | Vocal | −2 −2 −1 1 3 4 3 1 0 −1 |
| rock | Rock | 4 3 2 0 −1 −1 1 2 3 4 |
| pop | Pop | −1 0 2 3 4 3 1 0 −1 −1 |
| electronic | Electronic | 5 4 1 0 −2 1 0 2 4 5 |
| acoustic | Acoustic | 3 3 2 1 1 1 2 3 2 1 |
| night | Late Night | 4 3 1 0 −1 −1 0 1 2 3 |

Choosing a preset copies its bands. Moving any band switches the page to "Custom" while keeping the values.

## Settings page

`Page::Equalizer` sits in the first sidebar group after Sound. Icon `equalizer`, indigo tint `0x5e5ce6ff`.

1. **Hero** with an on/off switch on the right.
2. **Presets:** a wrapping row of chips. The active chip uses the accent colour. A "Custom" chip appears only when the bands match no preset.
3. **Bands:** 10 vertical sliders, labelled 31 … 16k below and with the dB value above. A 0 dB centre line runs across all of them. Double-clicking a slider resets it to 0. When the EQ is off the sliders still work but are drawn at 50 % opacity.
4. **Footer:** a note ("Boosts are balanced automatically so loud parts don't distort.") and a **Reset** button.

Looks, using the existing helpers (`corner`, `fill`, `frame`, `pixel_look`):

- **Glass:** each slider is a thin rounded track with an accent fill from the 0 dB line to the knob, and a round white knob with a soft shadow. The group card is translucent.
- **Pixel:** each slider is a column of 24 square cells (2 px gaps). Cells between 0 dB and the value are lit with the accent colour and the rest stay dim, like an LED meter. The knob is a square with the hard 3 px black shadow from `frame`. Text uses the pixel font.

## Out of scope

Per-device EQ profiles, user-saved presets, a response-curve graph, a Quick Settings tile, and microphone EQ.
