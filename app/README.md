# Choren studio

A studio compatible with Choregraphe 2.8 projects: `.pml` projects, `.crg` archives and standalone
`.xar` behaviors. Choren combines a dockable editor, box libraries and a virtual robot
while retaining imported coordinates, port semantics and link geometry.

## Use

- **Open project** loads one project at a time. A failed import leaves the current project intact.
  **Explore example** opens an original bundled sample without requiring a Choregraphe installation.
- **Project** shows declared files; **Outline** browses nested boxes, layers and keyframes.
  Search in Outline to reveal a box in its diagram and Inspector.
- **Project → ⋯ → Import behavior / Import files / Import folder** adds content to the current
  `.pml` project; the same actions are available in File and command search. Behavior import
  accepts a XAR definition or a CRG containing one behavior and its packaged resources.
  Loose XAR resources must be imported separately. File import accepts multiple files;
  folder import keeps the selected folder name and nested files, omitting empty folders.
  Choose a destination relative to the project root; existing names and symbolic links
  are rejected. Imports appear immediately and are written on **Save**. Files/folders
  support Undo; behavior imports use **Remove behavior**, like newly created behaviors.
- **Connection…** in the toolbar opens **Robot connection**: choose **Virtual robot** and
  a supported model (currently NAO 6), or **Real robot** with hostname/IP and port (9561 by
  default) and fills in the last robot it connected to. The dialog shows the current connection and **Disconnect**; disconnect before
  choosing another target. Connection failures keep the address for retry. The Robot panel,
  native Connection menu, and command search reach the same controls. Quick-start **Start
  virtual NAO 6** remains available. The real-robot tab lists discovered robots with Refresh
  and direct connection. Authentication/conflict dialogs and physical-hardware validation remain
  outstanding.
- **Robot view → Robot files** (also **Workspace → Show Robot files**) opens the SFTP browser.
  Connect with the robot's SSH address, username and password; it is independent of the
  behavior connection. **Upload file** sends a selected local file to the current folder.
  Browse folders, download selected files, create folders and delete items, with progress,
  cancellation and confirmation before replacement/deletion. Passwords are not saved.
- Click a box or connection to inspect it. Shift/Ctrl/Command-click selects multiple items.
  Double-click a flow/timeline box to enter it; double-click a script box to open its source.
  Breadcrumbs return to parent diagrams. Each graph remembers its viewport during the session.
- Space + drag pans. The canvas toolbar offers zoom, fit-all, fit-selection and a minimap.
  In editable projects, drag boxes, connect ports and apply script changes; undo and redo
  use the backend project history.
- Graph cards pair a color with an icon and category label: cyan Subgraph, purple Timeline,
  orange Timer, green Terminal, yellow Comment / Text, and slate Script / Custom. **Inspector → Properties →
  Appearance category** overrides automatic classification without changing behavior.
  Overrides are saved as the optional `choren_category` box attribute and support undo/redo.
  Imported stock Wait icons and the bundled Wait template identify timers after renaming;
  terminal roles are explicit. Timer cards show their duration and retain the parameter button.
  Cards grow to fit their ports. Saved positions are retained, so older, tightly packed
  diagrams may need manual spacing for the larger cards.
- **Settings → Graph UI** switches between **Choren UI** (colored cards, the default) and
  **Classic UI** (compact boxes and icons, as in Choregraphe). The preference applies to all
  open diagrams and is remembered across restarts. Switching retains positions, connections,
  selection, zoom, and category metadata; it does not edit the project. Both styles support
  the same linking, input activation, parameter controls, and nested navigation.
- While a behavior is running on the virtual robot, double-click a box's Start input
  (or focus it and press Alt+Enter) to run that individual box. This also works inside
  flows or timeline keyframes that have not been entered. Its output links run normally;
  enclosing Start rails are not triggered. Stop cleans up manually started boxes too.
- In **Settings**, **Virtual movement speed** changes virtual movement live from **0.25× to 2×**
  while a virtual robot is connected.
  **1× (Normal)** is the default for each new virtual robot. Joint motions, posture changes,
  walking and animation timeline frames share the speed override, so slowed gestures finish
  before their timeline's completion output. Wait boxes, Python sleeps, speech, audio and LED
  fades keep their original timing; this is not a global behavior time scale. The setting does
  not edit the project or affect physical robots. Reopening Settings retains the current
  robot's speed; starting a new virtual robot resets it.
- Imported Comment / Text Edit boxes display their own text in both graph styles and in the
  Inspector, preserving line breaks. Long comments scroll inside the box; drag its header
  to move it. Dropped boxes stay at their new position while the backend confirms the edit.
- **Settings → Choregraphe libraries** selects the installation's `share/choregraphe` directory.
  Libraries group categories into collapsible folders. Search reveals matching boxes;
  select a box to preview or insert it. Original icons load from that installation.
  Useful built-in symbols remain available without it. Vendor code/artwork is not bundled.
- Drag panel tabs to rearrange or combine them; drag dividers to resize. Each panel has
  maximize and actions buttons. Panel actions provide keyboard movement, resizing and closing.
  **Workspace → Show …** reopens panels; **Restore default layout** repairs an unwanted layout.
- F6 / Shift+F6 cycles panel-group focus. Command/Ctrl+Shift+P opens command search,
  Command/Ctrl+O opens a project, and Command/Ctrl+Shift+F opens Outline search.
  CodeMirror provides highlighted Python with line numbers and Command/Ctrl+F search.
- **Problems** lists import failures, missing declared files and unresolved connections.
  **Virtual** starts the local robot; its tools support running behaviors, recording motion,
  joint controls, logs, memory events and speech. **Workspace → Show Robot dialog** sends
  sentences to ALDialog, displays input/answer signals, changes language and lists active topics.

The default follows the system theme, with explicit light/dark options. Layout and preferences
persist; imported documents, scripts, selection and viewports are session-only. Native title
bars and menus are retained. The minimum logical window size is 1024×640.

## Additional authoring controls (2026-10-18)

- **File** includes Close project/tab, full Save As to an empty project folder and Build
  application package. Save As copies current applied documents and resources. Package building
  creates a ZIP `.pkg` in Rust; Play uploads it to virtual or real robots.
- **Project properties** edits localized titles/descriptions, supported languages, each behavior’s
  nature, permissions, launch conditions and localized lists, plus NAOqi and robot requirements.
  The raw manifest resource editor remains available for package contents.
- **Resource editor** previews images/audio/video, edits text/topic/TS files, renames resources
  and removes references. Text drafts survive panel switches and guard closing the project;
  changes apply to the project before Save. Renames update structured resource/bitmap paths,
  while paths embedded in scripts need manual edits. Remove reference keeps saved files.
- Right-click a diagram or port for editing actions; Shift+F10 opens a focused port's menu.
  Drop an output onto a box to choose a parameter. Timeline/curve controls include range,
  stretch, acquisition, mute, clipboard, key/handle dragging, transforms and Python/C++ export.
- **Libraries** imports `.cbl` archives into a new or empty folder and exports the selected user
  library with **Save as .cbl…**. It also supports user-box property/script edits, folder management and copying folders
  or definitions with attachments. Library edits save immediately. **Pose library → Properties**
  changes a pose's name, folder, description and image path while keeping the joint values.
- **Workspace** exposes Undo history, Service inspector, Camera, Robot applications and Active
  content. Active content polls behaviors/dialogs while open and keeps up to 500 finished
  observations; displayed timestamps are observation times. Physical camera/services require
  compatible NAOqi services. The virtual robot has no camera sensor.
- **Live parameters** in the toolbar enables **Auto-update parameters on robot** (off by default,
  remembered). Inspector value changes reach the running behavior with their declared types.
  Structural edits, undo/redo or a failed update require Play again; local edits remain in
  project history even if the robot rejects the update.
- Timeline and Motion curves display a separate robot playhead without moving the editing cursor.
  Mirror/Flip use selected key ranges, or the entire timeline when no keys are selected. Python
  motion export uses the Rust generator for both clipboard and file output; C++ stays local.
- **Settings** saves named panel layouts. The script editor has font/indent/wrap preferences,
  completion and a grammar syntax check. Memory watching adds discovered keys, sampling period,
  bounded recording and JSON export; robot logs can be filtered by severity and category.

Browser tests use simulated IPC and do not establish physical
robot compatibility, motion-code safety or native export-picker permissions.

## Develop and validate

Use Node 24 (tested), Rust and the Tauri prerequisites for your operating system. Running
behaviors on the virtual robot needs Python 3.8+ as `python3` on PATH.

```sh
npm ci
npm run tauri dev
npm run build
npm test
cargo test --workspace            # backend + virtual robot (from the repo root)
python3 -m unittest discover -s ../python/tests   # Python box host
npx playwright install chromium webkit
npm run test:ui
npm run tauri build -- --bundles app  # macOS
npm run tauri build                 # run on Windows for Windows installers
```

`npm run dev` is a browser preview with a bundled example. OS file dialogs and filesystem
access require the desktop app. Browser import tests use a deterministic IPC fixture; they
are not proof of native filesystem permissions or WebView2 behavior.

Corpus tests use `CHOREGRAPHE_SHARE` (default: your own Choregraphe 2.8.8 install, placed at the repository root as
`Choregraphe 2.8.8/`, which git ignores) and
`REF_PROJECT` (the reference fixture documented in `test/format.test.ts`). Missing corpora
are reported as skipped. Synthetic tests and browser workflows are independent of vendor files.

Layout rules: outside the graph, every text size and every width that holds text is
`calc(Npx * var(--text-scale))` (`test/styles.test.ts` enforces the font sizes); bars use
`min-height`, never `height`; buttons never wrap, so rows wrap instead. UI tests call
`expectTidyLayout(page, scope)` from `test/ui/layout-audit.ts` once a panel or dialog shows
realistic content; it fails on clipped or overflowing text, wrapped button labels, overlaps,
sideways scrolling, cut dropdown values and labels off-centre from their control, at 100% and
150% text. Set `CAPTURE_LAYOUT=<folder>` to save what it checked for a review by eye.

Validated on 2026-10-17: parser/geometry/workspace tests, Chromium and WebKit workflows,
production build and packaged macOS arm64 launch. macOS native file access was checked with
a Unicode project outside the home folder and the installed box libraries. Windows native
build, WebView2, installer behavior and physical 125–200% display scaling remain unverified.

## Source boundaries

| Location | Responsibility |
|---|---|
| `src/format/` | Immutable `ProjectDocument`, XAR/PML/CRG parsing and link resolution |
| `src/workspace/model.ts` | Reducer, complete graph addresses, selection, viewports, timeline state |
| `src/workspace/registry.ts` | Tool destinations, command definitions and availability explanations |
| `src/workspace/panels.tsx` | Project, Outline, Libraries, document, Timeline and Problems panels |
| `src/workspace/nativeMenu.ts` | Platform menus sharing the shell's command handlers |
| `src/workspace/ScriptViewer.tsx` | Lazy-loaded CodeMirror script editor and library source preview |
| `src/App.tsx` | Dockview layout, import lifecycle, command routing and workspace context |
| `src/app.css` | Shared visual/focus tokens and compact light/dark workspace styles |
| `src/diagram/` | Shared color/icon categories, card/port geometry, React Flow integration and graph interactions |
| `src/timeline/` | Viewport-bounded canvas, frame/layer navigation and scrolling |
| `src-tauri/` | Native shell, picker-scoped reads, project-root authorization, persisted scopes, and project sessions over `crates/choren-core` (`project_open/edit/undo/redo/save`) |

Dockview uses its free core and pointer dragging; no paid keyboard-navigation module or
separate native dock windows are required. Full original feature parity remains ongoing.
