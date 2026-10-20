/** Numeric references are opt-in in fast-xml-parser; use only XML's five named entities. */
export const xmlOptions = {
  ignoreAttributes: false,
  attributeNamePrefix: "",
  parseTagValue: false,
  parseAttributeValue: false,
  trimValues: false,
  // Runtime accepts a named-entity map; the upstream declaration still says boolean.
  htmlEntities: {
    amp: "&",
    lt: "<",
    gt: ">",
    quot: '"',
    apos: "'",
  } as unknown as boolean,
};

export const normalizePath = (path: string) => path.replace(/\\/g, "/");
export const basename = (path: string) =>
  normalizePath(path).split("/").at(-1) ?? path;
export const dirname = (path: string) =>
  normalizePath(path).replace(/\/[^/]*$/, "");

/** A project file as a Dialog box's `dialogFile` names it: relative to the behavior file's
 *  folder. */
export function behaviorRelative(behavior: string, path: string): string {
  const from = normalizePath(behavior).split("/").slice(0, -1), to = normalizePath(path).split("/");
  let i = 0;
  while (i < from.length && i < to.length - 1 && from[i] === to[i]) i++;
  return [...from.slice(i).map(() => ".."), ...to.slice(i)].join("/");
}

/** Imported references may only read within the root the user opened. */
export function relativePath(path: string): string {
  const normalized = normalizePath(path).replace(/^(\.\/)+/, "");
  if (
    !normalized ||
    normalized.startsWith("/") ||
    /^[a-z]:/i.test(normalized) ||
    normalized.split("/").includes("..")
  ) {
    throw new Error(`Reference is outside the project folder: ${path}`);
  }
  return normalized;
}
