**Application Architecture**
A reactive, state-driven architecture is ideal for a bidirectional MNX editor. The application core maintains a single source of truth: an in-memory **MNX Document Tree** (serialized/deserialized via Rust structs). Two primary views project this state: a raw text/code editor (modifying the MNX syntax directly) and a graphical rendering canvas (displaying the resulting guitar tab). Any change in the text editor parses into the MNX model, immediately triggering a layout recalculation and updating the tab view, and vice-versa.

**Recommended Rust Crates**

* **Data Serialization & Parsing:** `serde`, `serde_json`, `serde_yaml` to parse and emit MNX documents in both JSON and YAML formats.
* **GUI Framework:** `egui` (via `eframe`) for a lightweight, immediate-mode desktop application. It excels at building clean developer tooling, configuration menus, split-pane layouts, and custom 2D canvas renderers without heavy boilerplate. *(Alternative: `ratatui` if targeting a terminal-based TUI workflow).*
* **MIDI Integration:** `midir` to handle live hardware connections, allowing MIDI controllers or electronic instruments to stream note events directly into the active measure buffer.
* **Data Structures:** `indexmap` or custom tree structures to maintain ordered sequences of measures, parts, and notes matching the hierarchical MNX specification.

**Step-by-Step MVP Roadmap**

**1. Core Data Model & CLI Tab Renderer (Week 1)**

* Define Rust structs (`Score`, `Part`, `Measure`, `Sequence`, `Note`, `Tuning`) mirroring the MNX specification.
* Write a basic parser that ingests a YAML MNX file containing a custom tuning (e.g., standard or FACGCE) and a melody.
* Implement a pure calculation function that takes the MNX tree, applies the string/fret mapping rules, and outputs a formatted ASCII tab grid to the console.

**2. Basic GUI Shell & Split-Pane Editor (Week 2)**

* Initialize an `eframe`/`egui` desktop window with a top menu bar (File, Edit, Configuration).
* Create a split-pane layout: a text area on the left for editing raw MNX/YAML, and a rendering pane on the right.
* Wire up real-time bidirectional sync: typing valid MNX YAML updates the internal state struct, which instantly re-renders the visual tablature on the right panel.

**3. Configuration Menus & Custom Tuning (Week 3)**

* Build settings modals or side panels using `egui` widgets (`ui.slider`, `ui.combobox`) to configure:
* Number of strings (e.g., 6, 7, 8).
* Custom string tunings (mapping pitches to string indices).


* Dynamically update the layout engine's grid generator when the tuning configuration changes.

**4. MIDI Capture & Auto-Insertion (Week 4)**

* Integrate `midir` to listen for incoming note-on/note-off events from an attached MIDI keyboard or guitar controller.
* Map incoming MIDI pitch numbers against the current instrument tuning profile to automatically calculate the optimal string and fret coordinates.
* Append the generated note event directly into the active MNX measure sequence, closing the loop for automated score creation.