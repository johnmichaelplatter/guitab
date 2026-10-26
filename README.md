# guitab

**guitab** is a microtonal-ready guitar and fretted instrument tablature editor built on the W3C MNX specification, featuring real-time YAML editing, MPE and multi-timbral MIDI playback, and print-ready PDF/MNX export. Currently, only EDO-based guitars are supported, but support for arbitrarily/movable fretted guitars is in the works.

---

## How to Install

### Homebrew (macOS & Linux)
```bash
brew install johnmichaelplatter/tap/guitab
```

### Prebuilt Binaries (macOS, Windows, Linux)
Download the latest prebuilt archive for your platform from the [GitHub Releases page](https://github.com/johnmichaelplatter/guitab/releases), extract the archive, and run `guitab`.

### From Source (Cargo)
```bash
cargo install --git https://github.com/johnmichaelplatter/guitab
```

---

## New File

Currently **guitab** is designed for single instrument part editing and playback. 

Set up the instrument's number of strings, Equal Divisions of the Octave (EDO), and open string tuning. You can leverage [Kite's "ups and downs"](https://en.xen.wiki/w/Kite%27s_ups_and_downs_notation) notation for naming string notes in microtonal instruments, or use the numerical `Step` number to specify absolute EDO steps from C.

Be sure to set the octave of each string. Current convention specifies A4 = 440Hz, and octave numbers change such that C4 directly follows B3. 

---

## Note Input

`guitab` uses two cursor modes on the tablature staff: **Select Mode** (blue highlight) and **Edit Mode** (orange highlight).

- **Navigation**:
  - `Left` / `Right` arrows move across rhythmic events; `Up` / `Down` arrows move across strings.
  - Pressing `Right` at the end of the score automatically adds the next note according to the selected rhythm duration
  - Use `Shift` + `Left` / `Right` (or + click) to select a range of events.
    - Events can be Cut/Copied/Pasted/Duplicated with standard hotkeys
    - Additional hotkeys for convenience include Cut Time (`Shift+Ctrl+X`), Insert Paste (`Shift+Ctrl+V`), and Insert Duplicate (`Shift+Ctrl+D`). These operations change the timing of all following notes, but sometimes that is what you want when you're composing!
- **Entering Notes**:
  - Press `Enter` or start typing digits (`0`–`9`) on a selected note to enter **Edit Mode** (orange highlight).
  - Type the fret number (or `X` for a muted string) and press `Enter` or an arrow key to accept the note.
  - Press `Backspace` or `Delete` to clear a note or rest.
- **Durations & Rhythm**:
  - Set note durations with number keys outside orange edit mode:
    - `8`: Whole note
    - `7`: Half note
    - `6`: Quarter note
    - `5`: Eighth note
    - `4`: 16th note
    - `3`: 32nd note
  - `.` (Period): Toggle augmentation dot.
  - `R`: Convert the selected duration to a rest.
  - Use the toolbar to toggle **Tuplets** and **Grace Notes**, or add Tempo markings/changes.
- **Techniques & Articulations**:
  - `Shift+T`: Tie to the next note on the same string
  - `S`: Hammer-on / Pull-off (slur)
  - `T`: Tap
  - `L`: Slide to the next note on the same string
  - `B`: Add a bend to the note. Opens the interactive **Bend Curve Editor** which displays edo step gridlines (based on the current pitch bend settings).
  - Additional toolbar buttons: **Natural Harmonic** (`<N.H.>`), **Accent**, **Arpeggio Roll** (Up/Down), **Strum** (Up/Down), **Time Signatures**, **Tempo / rit. & accel.**, and **Repeats / Endings**. Click after selecting a note.
- **Lyrics** (`Shift+L`):
  - Type word/syllable and hit OK/Enter to add a lyric to a duration
  - Simply hit `Space` to automatically move to the next duration
  - To extend a syllable, type '-'
  - To extend a single word/final syllable, type '_'
  - Multi-line lyrics are supported. Switch between lines with Up/Down.

---

## MIDI Settings and Playback

`guitab` sends real-time MIDI to any virtual or hardware synthesizer on your system, with full microtonal pitch-bend support.

- **Playback Controls**:
  - Click **▶ Play** or press `P` to play from the currently selected note (`Shift+P` plays from the start of the score).
  - Click **⏹ Stop** or press `Space` / `Esc` to stop playback.
- **MIDI Output Device**:
  - Select your target synth or virtual MIDI port (e.g., IAC Driver on macOS) from the **MIDI** toolbar dropdown. Click `⟳` to refresh available ports.
- **Multi-Timbral vs. MPE Mode** (toggle button):
  - **Multi-Timbral Mode** *(default)*: Assigns one MIDI channel per string (`Ch 1..N`) with a default pitch bend range of `±2` semitones, ensuring independent per-string microtonal tuning, bends, and slides.
  - **MPE Mode**: Defaults to MPE-friendly defaults: `Ch 2..N+1` (reserving Channel 1 as the MPE Master channel) with a `±48` semitone pitch bend range. Note: this impacts string bend capability - you may want to play around with this setting in your synth.
  - These default settings can be altered with the **PB Range** number box & **String Channel** mapping dialogue.

---

## Layout vs Continuous View Mode

Switch between views anytime via **View → Layout Mode / Continuous Mode**:

- **Continuous Mode**:
  - A single horizontal scrolling timeline designed for fast composition, note entry, and playback tracking.
- **Layout Mode**:
  - A WYSIWYG paginated print preview (`Letter` or `A4`). Export this view as a PDF with **File → Export → PDF**.
  - Click and drag individual score elements (lyrics, tempo markings, directions) to fine-tune positioning, or press **Reset Selected Offset(s)** to restore defaults.
  - **System & Page Formatting**: Add system breaks, page breaks, and bar number styles (**View → Bar Numbers**). Also, you may `Shift` + click two measures and click **Make into System** to lock a range of bars onto a single line.
  - To add details about the piece like Composer, Title and Subtitle, go to **Edit → Score Information**.

---

## Instrument Setup
The tuning of a guitar can change at any time and guitab will attempt to fit the old music onto the new tuning. This can be used to 
- convert music between EDOs
- play the same music with a different open string tuning

If no acceptable fit can be found, guitab will highlight "impossible" notes in new instrument setups. `Ctrl+Z` can always be leveraged to undo any music conversion. 

---

## MNX YAML Editor

The score's underlying data structure is based on W3's evolving MNX standard. Toggle the live source panel via **View → Show/Hide MNX YAML editor**:

- **Two-Way Live Sync**: Every action in the tablature GUI updates the MNX YAML document in real time, and any edit typed into the YAML panel immediately re-validates and re-renders the tablature.
- **Selection Highlighting**: The currently selected event or chord note in the tablature is automatically highlighted in the YAML source (blue in Select Mode, orange in Edit Mode).
- **Rhythm & Syntax Validation**: If manual YAML edits introduce syntax errors or measures whose durations do not match the time signature, `guitab` highlights the affected bars and displays detailed diagnostics in the bottom **Validation Errors** panel.
