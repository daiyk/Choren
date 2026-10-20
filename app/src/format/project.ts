import { XMLParser } from "fast-xml-parser";
import { inflateSync } from "fflate";
import { parseXar } from "./xar";
import type { ProjectDocument, ProjectFile } from "./types";
import {
  basename,
  dirname,
  normalizePath,
  relativePath,
  xmlOptions,
} from "./xml";

export type ReadFile = (path: string) => Promise<Uint8Array>;
export type FileExists = (path: string) => Promise<boolean>;
const utf8 = new TextDecoder();
const listTags = new Set([
  "BehaviorDescription",
  "File",
  "Dialog",
  "Topic",
  "Translation",
  "name",
]);
const projectParser = new XMLParser({
  ...xmlOptions,
  isArray: (name, _path, _leaf, attribute) => !attribute && listTags.has(name),
});

/** Opens a project without changing the caller's current document on failure. */
export async function openProject(
  path: string,
  readFile: ReadFile,
  exists?: FileExists,
): Promise<ProjectDocument> {
  const source = normalizePath(path);
  const ext = source.toLowerCase().split(".").pop();
  if (ext !== "pml" && ext !== "crg" && ext !== "xar")
    throw new Error(`Unsupported file type: .${ext}`);
  const bytes = await readFile(path);
  const doc: ProjectDocument = {
    id: source,
    source,
    name: basename(path).replace(/\.[^.]+$/, ""),
    format: ext,
    behaviors: [],
    files: [],
    metadata: {},
    diagnostics: [],
  };
  const addBehavior = (name: string, file: string, data: Uint8Array) => {
    const id = `${source}::${file}`;
    doc.behaviors.push({
      id,
      source: file,
      name,
      root: parseXar(utf8.decode(data)),
    });
    doc.files.push({ path: file, kind: "behavior", behaviorId: id });
  };
  if (ext === "xar") addBehavior(doc.name, basename(path), bytes);
  if (ext === "crg") {
    for (const [raw, data] of crgFiles(bytes)) {
      const file = relativePath(raw);
      if (file.endsWith(".xar"))
        addBehavior(file.replace(/\/behavior\.xar$/, ""), file, data);
      else
        doc.files.push({
          path: file,
          kind: file.endsWith("manifest.xml") ? "manifest" : "resource",
        });
    }
  }
  if (ext === "pml") {
    const pml = projectParser.parse(utf8.decode(bytes)).Package;
    if (!pml || typeof pml !== "object")
      throw new Error("Not a Choregraphe project (no Package)");
    doc.name = pml.name || doc.name;
    doc.metadata["Format version"] = pml.format_version ?? "Unknown";
    doc.files.push({ path: basename(path), kind: "project" });
    const root = dirname(source);
    for (const d of pml.BehaviorDescriptions?.BehaviorDescription ?? []) {
      const file = relativePath(`${d.src}/${d.xar}`);
      addBehavior(d.src, file, await readFile(`${root}/${file}`));
    }
    const sections = [
      ["Resources", "File", "resource"],
      ["Dialogs", "Dialog", "dialog"],
      ["Topics", "Topic", "topic"],
      ["Translations", "Translation", "translation"],
    ] as const;
    for (const [section, tag, kind] of sections) {
      for (const item of pml[section]?.[tag] ?? [])
        doc.files.push({ path: relativePath(item.src), kind });
    }
    if (pml.Manifest?.src) {
      const file = relativePath(pml.Manifest.src);
      const entry: ProjectFile = { path: file, kind: "manifest" };
      doc.files.push(entry);
      try {
        const manifest = projectParser.parse(
          utf8.decode(await readFile(`${root}/${file}`)),
        ).package;
        if (manifest?.version) doc.metadata.Version = manifest.version;
        if (manifest?.uuid) doc.metadata.UUID = manifest.uuid;
        const names = manifest?.names?.name ?? [];
        const title =
          names.find((n: { lang?: string }) => n.lang?.startsWith("en")) ??
          names[0];
        if (title?.["#text"]) doc.metadata.Title = title["#text"];
      } catch {
        entry.missing = true;
        doc.diagnostics.push({
          id: file,
          severity: "warning",
          path: file,
          message: "Project manifest is missing or unreadable.",
        });
      }
    }
    if (exists) {
      await Promise.all(
        doc.files
          .filter((f) => !["behavior", "project", "manifest"].includes(f.kind))
          .map(async (file) => {
            try {
              file.missing = !(await exists(`${root}/${file.path}`));
            } catch {
              file.missing = true;
            }
            if (file.missing)
              doc.diagnostics.push({
                id: file.path,
                severity: "warning",
                path: file.path,
                message: "Referenced file is missing or inaccessible.",
              });
          }),
      );
    }
  }
  return doc;
}

/** .crg = Debian "ar" archive whose data.tar(.gz) member holds behavior.xar. */
export function crgFiles(crg: Uint8Array): [string, Uint8Array][] {
  const data = arMember(crg, "data.tar.gz");
  const tar = data[0] === 0x1f && data[1] === 0x8b ? gunzipPadded(data) : data;
  return tarFiles(tar);
}

// Choregraphe pads the gzip member with zeros to 10 KiB blocks, which breaks gunzipSync (it reads
// the size from the last 4 bytes), so skip the header and inflate the deflate stream directly.
function gunzipPadded(d: Uint8Array): Uint8Array {
  const flags = d[3];
  let p = 10;
  if (flags & 4) p += 2 + (d[10] | (d[11] << 8));
  if (flags & 8) while (d[p++]);
  if (flags & 16) while (d[p++]);
  if (flags & 2) p += 2;
  return inflateSync(d.subarray(p));
}

function arMember(ar: Uint8Array, wanted: string): Uint8Array {
  if (utf8.decode(ar.subarray(0, 8)) !== "!<arch>\n")
    throw new Error("Not an ar archive");
  for (let off = 8; off + 60 <= ar.length;) {
    const name = utf8
      .decode(ar.subarray(off, off + 16))
      .trim()
      .replace(/\/$/, "");
    const size = parseInt(
      utf8.decode(ar.subarray(off + 48, off + 58)).trim(),
      10,
    );
    if (!Number.isSafeInteger(size) || size < 0 || off + 60 + size > ar.length)
      throw new Error("Invalid archive member size");
    if (name === wanted) return ar.subarray(off + 60, off + 60 + size);
    off += 60 + size + (size % 2);
  }
  throw new Error(`${wanted} not found in archive`);
}

function tarFiles(tar: Uint8Array): [string, Uint8Array][] {
  const out: [string, Uint8Array][] = [];
  for (let off = 0; off + 512 <= tar.length;) {
    const name = utf8
      .decode(tar.subarray(off, off + 100))
      .replace(/\0.*$/s, "");
    if (!name) break;
    const size = parseInt(
      utf8
        .decode(tar.subarray(off + 124, off + 136))
        .replace(/\0.*$/s, "")
        .trim() || "0",
      8,
    );
    if (
      !Number.isSafeInteger(size) ||
      size < 0 ||
      off + 512 + size > tar.length
    )
      throw new Error("Invalid archive file size");
    const type = tar[off + 156];
    if (type === 0 || type === 0x30)
      out.push([
        name.replace(/^\.\//, ""),
        tar.subarray(off + 512, off + 512 + size),
      ]);
    off += 512 + Math.ceil(size / 512) * 512;
  }
  return out;
}
