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

## Instrument Setup & Multi-Instrument Scores

`guitab` supports both single-instrument tabs and full multi-instrument arrangements.

- **Instrument Configuration**: Set up each instrument's number of strings, Equal Divisions of the Octave (EDO), custom fretboard, and open string tuning. You can leverage [Kite's "ups and downs"](https://en.xen.wiki/w/Kite%27s_ups_and_downs_notation) notation for naming string notes in microtonal instruments, or use numerical EDO steps from C.
- **Octave Conventions**: A4 = 440 Hz reference, with octave numbers incrementing at C (C4 follows B3).
- **Multi-Instrument Arrangements**:
  - Add, switch, rename, or reorder instruments via the instrument dropdown in the top toolbar or through **Instrument Setup...**.
  - **Ensemble & Part Views in Layout Mode**: Toggle between the full **Ensemble** score (grouped with system brackets and instrument names) or view individual instrument parts for isolated editing and PDF export. 

---

## Note Input

`guitab` uses two cursor modes on the tablature staff: **Select Mode** (blue highlight) and **Edit Mode** (orange highlight).

- **Navigation & Transposition**:
  - `Left` / `Right` arrows move across rhythmic events; `Up` / `Down` arrows move across strings.
  - Pressing `Right` at the end of the score automatically adds the next note according to the selected rhythm duration.
  - Use `Shift` + `Left` / `Right` (or + click) to select a range of events.
    - Events can be Cut/Copied/Pasted/Duplicated with standard hotkeys.
    - Additional hotkeys for convenience include Cut Time (`Shift+Ctrl/⌘+X`), Insert Paste (`Shift+Ctrl/⌘+V`), and Insert Duplicate (`Shift+Ctrl/⌘+D`). These operations change the timing of all following notes.
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
  - `Shift` + `Up` / `Down`: Shift fret numbers on selected notes up or down by 1 fret.
  - `Alt`/`⌥` + `Up` / `Down`: Move selected notes up or down strings.
  - `Alt`/`⌥` + `Left` / `Right`: Nudge selected notes earlier or later in time by the **Current Grid** division (e.g. 1/8, 1/16, etc...).
  - `Shift` + `Alt`/`⌥` + `Up` / `Down`: Nudge note durations up (longer) or down (shorter) through standard rhythmic values (whole ↔ half ↔ quarter ↔ 8th ↔ 16th ↔ 32nd).
  - `Ctrl`/`⌘` + `Up` / `Down`: In multi-instrument scores, jump the selection cursor up or down instruments.
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

`guitab` sends real-time MIDI to any virtual or hardware synthesizer on your system, with full microtonal pitch-bend support and multi-instrument routing.

- **Playback Controls**:
  - Click **▶ Play** or press `P` to play all parts from the currently selected note (`Shift+P` plays all parts from the start of the score).
  - Press `Ctrl`/`⌘`+`P` to solo playback only the currently active instrument part from the selected note (`Shift`+`Ctrl`/`⌘`+`P` solos from the start).
  - Click **⏹ Stop** or press `Space` / `Esc` to stop playback.
- **Global & Per-Instrument MIDI Configuration**:
  - Click the **MIDI/String Channels** dropdown in the toolbar to configure playback settings.
  - In multi-instrument scores, use the instrument dropdown at the top of the popup to configure each instrument independently:
    - **Multiple MIDI Output Ports**: `guitab` can route each instrument part to its own dedicated MIDI device or virtual port (e.g., Lead Guitar to IAC Driver Bus 1, Rhythm Guitar to IAC Driver Bus 2, Bass to an external hardware synth). Click `⟳` to refresh available ports.
    - **Per-Instrument PB Range, Multi-Timbral/MPE**: Set pitch bend range (`±1` to `±48` semitones) and  Multi-Timbral and MPE toggle independently for each instrument to match the output synth.
      - **MPE String Limit Warning**: If more than 15 total strings share a port in MPE mode, a warning indicator prompts you to assign a second MIDI port.
    - **Per-String Channel Mapping**: Detailed table to map String 1..N to specific MIDI channels, with one-click buttons to reset to default channel mappings. Multiple instruments can safely share a single MIDI port, just be sure not to use the same channel on a given port twice.

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

_Tip: change duration with the number keys before adding the notes from the grid controller, and use `Ctrl/⌘+Z` to undo a wrong chord from the grid input!_

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

If no acceptable fit can be found, `guitab` highlights "impossible" notes. `Ctrl`/`⌘`+`Z` can always be leveraged to undo any music conversion.

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

---

## Changelog

### v0.8.2 (Most Recent)
- **Multi-Instrument Arrangements**:
  - Full support for adding, editing, reordering, and deleting multiple instrument tabs in a single score.
  - **Stacked Continuous View**: All instrument staves are rendered vertically stacked with synchronized measure widths and common beat lines.
  - **Ensemble vs. Part Views**: Layout Mode provides both full "Ensemble" view (with system brackets and instrument labels) and isolated individual instrument tabs for printing and PDF export.
  - **Per-Instrument MIDI Routing & Multi-Port Output**:
    - Assign distinct MIDI output devices per instrument (enabling multi-port routing to different hardware synths or virtual buses simultaneously).
    - Independent pitch bend range (`±1` to `±48` st) and operating mode (Multi-Timbral vs. MPE) per instrument.
  - New solo playback shortcuts (`Ctrl`/`⌘`+`P` to play from selected note, `Shift`+`Ctrl`/`⌘`+`P` to play from start).
- **Navigation & Workflow Shortcuts**:
  - **Jump Between Parts** (`Ctrl`/`⌘` + `Up` / `Down`): Instantly jump between adjacent instrument staves while retaining your exact rhythmic position in the timeline.
  - **Duration Nudge** (`Shift` + `Alt`/`⌥` + `Up` / `Down`): Step note durations up or down through standard rhythmic values (whole, half, quarter, 8th, 16th, 32nd) in both Select Mode and Edit Mode.
- **Bug fixes**:
  - Bug fixes re: slur/tie behavior
  - "Let ring" ties render correctly in MIDI without premature note-offs
  - Tempo markins can now be deleted/edited
  - Special barlines (repeats, 1st/2nd endings) behavior when copy/pasting fixed
  - Fixed EDO tuning on custom fretboards
  - Moved lyric extenders to standard MNX spec; corrected visual rendering for extenders
  - Grid controller callibration preserved with app data

### v0.8.0
- **Custom Fretboard & Just Intonation Fretlet Editor**:
  - Visual interactive neck editor supporting JI ratios, cents, and EDO intervals.
  - Partial fretlet placement, double-click full frets, right-click removal, custom fret renumbering (e.g. 3.5), and auto note naming.
- **Hardware MIDI Grid Controller Integration**:
  - Connect up to 4 simultaneous MIDI grid controllers (Novation Launchpads, LinnStrument, Push, etc.).
  - 4-corner calibration, per-controller fret offsets, 0.5s chord latching, and "Just Play" audition mode.
- **Instrument Presets & Library**:
  - Pre-configured instrument setups for popular EDO systems (12, 19, 22, 31, Kite guitar) and JI tunings.
  - Preset export and import via YAML files.
- **Score Information**: Added title, subtitle, and composer metadata fields in Layout Mode.
- **Stability & Performance**: CoreMIDI sleep recovery fixes and rendering optimizations.

### v0.7.1
- **Cold Start Open Dialogue**: Graceful open/new tab dialogue on launch when default files are not found, avoiding crash or error screens.
- **Documentation**: Initial comprehensive README guide and installation documentation.

### v0.7.0
- **Initial Public Beta Release**:
  - Microtonal guitar tablature editor built on W3C MNX specification with real-time two-way YAML editing.
  - Graphical tablature UI built with `egui` featuring Select Mode (blue) and Edit Mode (orange).
  - Real-time MIDI playback with Multi-Timbral (channel per string) and MPE microtonal pitch bends.
  - Full tablature notation: bends (interactive curve editor), slides, taps, hammer-ons/pull-offs, ties, natural harmonics, accents, strums, arpeggio rolls, muted notes, tuplets, grace notes, repeat barlines, and 1st/2nd endings.
  - Multi-line lyrics entry with hyphens and extenders.
  - Continuous editing mode and paginated WYSIWYG Layout Mode with drag-and-drop offset adjustments.
  - Print-ready PDF and MNX export.
