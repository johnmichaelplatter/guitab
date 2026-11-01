# guitab

**guitab** is a microtonal-ready guitar and fretted instrument tablature editor built on the W3C MNX specification, featuring real-time YAML editing, MPE and multi-timbral MIDI playback, MIDI grid controller input, custom fretboard & Just Intonation fretlet editing, and print-ready PDF/MNX export.

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

- **Navigation & Transposition**:
  - `Left` / `Right` arrows move across rhythmic events; `Up` / `Down` arrows move across strings.
  - Pressing `Right` at the end of the score automatically adds the next note according to the selected rhythm duration.
  - Use `Shift` + `Left` / `Right` (or + click) to select a range of events.
    - Events can be Cut/Copied/Pasted/Duplicated with standard hotkeys.
    - Additional hotkeys for convenience include Cut Time (`Shift+Ctrl+X`), Insert Paste (`Shift+Ctrl+V`), and Insert Duplicate (`Shift+Ctrl+D`). These operations change the timing of all following notes.
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
- **Quick Edit Shortcuts**:
  - **Fret Shifts** (`Shift` + `Up` / `Down`): Shift fret numbers on selected notes up or down by 1 fret.
  - **String Movement** (`Alt` + `Up` / `Down`): Move selected notes up or down strings.
  - **Rhythm Movement** (`Alt` + `Left` / `Right`): Nudge selected notes earlier or later in time by the **Current Grid** division (e.g. 1/8, 1/16, etc...)
- **Techniques & Articulations**:
  - `Shift+T`: Tie to the next note on the same string
  - `S`: Hammer-on / Pull-off (slur)
  - `T`: Tap
  - `L`: Slide to the next note on the same string
  - `B`: Add a bend to the note. Opens the interactive **Bend Curve Editor** which displays edo step gridlines (based on the current pitch bend settings).
  - `H`: Natural Harmonic (`<N.H.>`)
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

## Grid Controller Input

`guitab` supports hardware MIDI grid controllers (such as Novation Launchpads, LinnStrument, Ableton Push, or generic multi-row pad controllers) for fast tablature entry and live auditioning.

### Multi-Controller Setup & Dropdown Panel
- Click the "**Grid Controllers**" dropdown in the top toolbar.
- Connect up to **4 simultaneous grid controllers** independently.
- **Fret Offset** (`-` / `+`): Shift the fret range covered by each controller, enabling multiple grid controllers to span consecutive registers of the fretboard (e.g., Controller 1 for frets 0–8, Controller 2 for frets 9–17).

### Calibration
When you first use a new grid controller, be sure calibrate in the Grid Controller dropdown. You should only have to do this once. 

### Grid Input Mode (Light-Red Highlight)
Striking pads on a connected, calibrated grid controller activates the red **Grid Input Mode**. After waiting 0.5s, the notes will be written into the tab and move you over to the next note for input. 

_Tip: change duration with the number keys before adding the notes from the grid controller, and use `Ctrl+Z` to undo a wrong chord from the grid input!_

### "Just Play" Audition Mode
To bypass red **Grid Input Mode**, click the yellow **Just Play** button next to the Grid Controllers menu (active when yellow). Use **Just Play** mode to freely jam, explore alternate tunings, or practice chord shapes on your grid.

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

## Instrument Setup & Custom Fretboard Editor
Click **Instrument Setup...** in the top bar to configure open string tunings, pitch reference frequencies, and custom fretboards.

The open string tuning of a guitar can change at any time and `guitab` will attempt to fit existing music onto the new tuning. This can be used to:
- Convert music between different EDOs (Equal Divisions of the Octave)
- Play the same music with different open string tunings or capos
- Adapt standard music to microtonal or Just Intonation fretboards

If no acceptable fit can be found, `guitab` highlights "impossible" notes. `Ctrl+Z` can always be leveraged to undo any music conversion.

### Custom Fretboard & Fretlet Editor
Click the "Custom Fretboard Editor" button in instrument setup to create non-EDO fretboards. Essentially any fretboard can be accomodated in this setup.
- Type a valid interval into the "Dotted Guide Grid:" box. This can be 
  - a **JI ratio**, like 5/4 (has a fraction slash "/")
  - a **cents value**, like 130.0 (has a decimal)
  - an **EDO step**, like 1\12 (has a backslash "\\", interpreted as "1 step of 12 EDO")
- **Placing Frets/Fretlets**: Click anywhere on the blue dotted line to add a fretlet for that string. Double-click on a blue dotted line to place a full fret at that physical location. Right-click to remove frets or parts of frets.
- **Re-Numbering Frets**: Full frets are auto-counted up a whole number fret. Occasionally, you may want a certain fret to be interpreted as a "partial fret," e.g. 3.5 if it's inbetween the 3rd and 4th fret. To override the fret number, click on the fret's number above the fretboard and type your desired number.
- **Note Names** are automatically inferred based on the resulting interval but you can override the note name (with respect to C) in the bottom fret list.

### Instrument Presets & Library Export / Import
- **Built-in Presets**: Pre-configured instrument setups for popular EDO systems (12, 19, 22, 31, Kite fretting) and Just Intonation tunings.
- **Saving & Loading Presets**: Save custom instruments and fretboards to your local preset library or load existing presets with a single click.
- **Export & Import YAML**: Export single presets or entire preset libraries to YAML files, making it easy to share custom microtonal or fretlet layouts with other musicians.

---

## MNX YAML Editor

The score's underlying data structure is based on W3's evolving MNX standard. Toggle the live source panel via **View → Show/Hide MNX YAML editor**:

- **Two-Way Live Sync**: Every action in the tablature GUI updates the MNX YAML document in real time, and any edit typed into the YAML panel immediately re-validates and re-renders the tablature.
- **Selection Highlighting**: The currently selected event or chord note in the tablature is automatically highlighted in the YAML source (blue in Select Mode, orange in Edit Mode).
- **Rhythm & Syntax Validation**: If manual YAML edits introduce syntax errors or measures whose durations do not match the time signature, `guitab` highlights the affected bars and displays detailed diagnostics in the bottom **Validation Errors** panel.
