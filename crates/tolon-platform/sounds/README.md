# Bundled alarm sounds

| File | Length | Size | What it is |
|---|---|---|---|
| `soft-bell.wav` | 1.5 s | 65 KB | One soft bell strike, D5, slow decay. |
| `chime.wav` | 1.0 s | 43 KB | Two short notes, E5 then B5. |

Mono, 22.05 kHz, 16-bit PCM WAV. They are embedded in the binary with `include_bytes!` (see `src/sound.rs`), so the app needs no file paths at run time.

## Licence

Both files are original work, synthesised by `src/synth.py` from sine waves. No recording, sample or third-party audio is used. They are licensed under Apache-2.0, like the rest of the repository.

## Regenerate

```
python3 crates/tolon-platform/sounds/src/synth.py
```

The script uses only the Python standard library and has no random input, so the same Python gives the same bytes. Rebuild after a change; the Rust tests check the length and the peak level of each file.
