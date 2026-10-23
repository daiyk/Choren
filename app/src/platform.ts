import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { readDir, readFile } from "@tauri-apps/plugin-fs";
import type { Box, ProjectDocument } from "./format/types";
import { parseXar } from "./format/xar";

export const native = isTauri();
export const mac = /Mac/.test(navigator.platform);
export const mod = mac ? "⌘" : "Ctrl+";

export async function chooseProject() {
  if (!native)
    throw new Error(
      "Open project is available in the desktop app. Explore the example in this preview.",
    );
  const path = await open({
    filters: [
      { name: "Choregraphe project", extensions: ["pml", "crg", "xar"] },
    ],
  });
  if (!path) return undefined;
  await invoke("authorize_project_root", { path });
  // The Rust backend owns the lossless model; the TS parser only serves the browser preview.
  return invoke<ProjectDocument>("project_open", { path });
}

/** Frees the backend session of a project the workspace no longer shows. */
export function releaseProject(project?: ProjectDocument) {
  if (native && project?.session)
    invoke("project_close", { session: project.session }).catch(() => {});
}

export async function chooseLibrary() {
  if (!native)
    throw new Error("Select the Choregraphe installation in the desktop app.");
  const picked = await open({
    directory: true,
    recursive: true,
    title: "Select Choregraphe (the app or its share/choregraphe folder)",
  });
  // The backend finds share/choregraphe from any installation level and refuses other folders.
  return picked ? invoke<string>("library_share_dir", { path: picked }) : null;
}

export interface LibraryEntry {
  name: string;
  path: string;
  category: string;
  /** Backend only: "install", "choren" (bundled clean-room library) or "user". */
  source?: string;
  root?: string;
  relativePath?: string;
}
export interface LibraryTreeNode {
  name: string; relativePath: string; kind: "folder" | "box";
  boxFile: string | null; hasScript: boolean; children: LibraryTreeNode[];
}
export interface LibraryRoot {
  path: string; name: string; source: string; writable: boolean; children: LibraryTreeNode[];
}
export const libraryTree = (shareDir?: string | null, userDirs: string[] = []) =>
  invoke<LibraryRoot[]>("library_tree", { shareDir: shareDir || null, userDirs });
export async function indexLibrary(
  root?: string | null,
  userDirs: string[] = [],
): Promise<LibraryEntry[]> {
  // The backend orders by xalinfo, includes behavior.xar boxes and adds the Choren library.
  if (native)
    return invoke<LibraryEntry[]>("library_index", {
      shareDir: root || null,
      userDirs,
    });
  if (!root) return [];
  const entries: LibraryEntry[] = [];
  const walk = async (relative: string) => {
    for (const item of await readDir(`${root}/${relative}`)) {
      if (
        item.isDirectory &&
        !item.isSymlink &&
        !item.name.startsWith(".") &&
        item.name !== "translations"
      )
        await walk(`${relative}/${item.name}`);
      else if (item.name === "box.xar") {
        const parts = relative.split("/").slice(2);
        entries.push({
          name: parts.at(-1)!,
          category: parts.slice(0, -1).join(" / "),
          path: `${root}/${relative}/box.xar`,
        });
      }
    }
  };
  await walk("libraries/box");
  return entries.sort(
    (a, b) =>
      a.category.localeCompare(b.category) || a.name.localeCompare(b.name),
  );
}

export async function readLibraryBox(path: string) {
  if (native) return invoke<Box>("library_box", { path });
  return parseXar(new TextDecoder().decode(await readFile(path)));
}

/** Internal library drag payload; the backend still validates access to the box path. */
export const LIBRARY_BOX_MIME = "application/x-choren-library-box";
export const PROJECT_FILE_MIME = "application/x-choren-project-file";
