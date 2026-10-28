# Changelog

Release notes are extracted from the section matching the version in
`app/src-tauri/tauri.conf.json`. Keep entries newest first, using
`## [version] - YYYY-MM-DD` headings. Dates below follow the tagged commits;
this file does not indicate whether a GitHub release has been published.

## [0.3.1] - 2026-10-28

- Delete folders from the Project tree, and choose whether deleting a behavior or folder
  also deletes its files. Files are deleted when the project is saved; Undo restores them.
- Add Rename, Duplicate, Move up/down, Copy name and Copy path for behaviors and folders in
  the Project and Outline menus, Rename box for Outline boxes, and Rename project.
- Drag behaviors and folders to reorder them, or onto a folder to move them there. Paths,
  dialog files and resource references follow the move, and files move on save. Scripts
  that start a behavior by its path are not updated.
- Fix Import folder and Import files listing every file as a plain resource: a
  `behavior.xar` now becomes a behavior that shows in the Outline and opens as a graph,
  and `.dlg` and `.top` files become dialogs and topics.
- Redesign the import dialogs with a source card, an "Imported as" preview of the
  destination, and the main action on the right.
- Fix every tab's close button being named "Close tab" (now "Close <panel>"), and the
  Timeline showing no layer rows at the minimum window size.

## [0.3.0] - 2026-10-25

- Prepare Windows x64 builds: an NSIS installer (`tauri.windows.conf.json`) and LF line
  endings in every checkout, so Windows builds bundle the same files as macOS.
- Build the Windows installer in the release workflow, next to the macOS builds, with
  installation notes for both platforms in the draft release.
- Fix the bundled Windows Python runtime missing `threading`, a console window opening
  for each behavior run on Windows, and license-notice generation failing on Windows
  systems whose default encoding is not UTF-8.
- Add an About Choren window with the app version, copyright and license, opened from
  Help on Windows and the Choren menu on macOS. It replaces the macOS system panel,
  which showed version 0.1.0.

## [0.2.0] - 2026-10-23

- Add target-specific right-click menus for diagram connections, boxes, selections,
  Project, Outline, Libraries, dock tabs, editors, and robot panels.
- Support keyboard menu access, arrow-key navigation, focus restoration, and menu
  placement beyond small dock boundaries.
- Display empty library folders and keep library roots and filesystem paths distinct.
- Preserve selected groups and project undo behavior, and reject outdated targets
  after document or connection changes.
- Improve timeline and motion-curve targeting, including occupied-frame guards and
  protection against unintended dragging with the right mouse button.
- Update release automation for Node 24 and updater metadata generation.
- Populate new draft release descriptions from this changelog, retaining the
  installation instructions. Reject missing or empty release entries before building.

## [0.1.0] - 2026-10-20

- Introduce the desktop studio with project browsing, visual behavior diagrams,
  timeline and motion editing, script editing, and box libraries.
- Include virtual robot support, robot inspection tools, and an independent SFTP
  file browser.
- Provide macOS release builds with optional signing and updater bundles.
- Distribute the project under BSD-3-Clause and include third-party license notices.
