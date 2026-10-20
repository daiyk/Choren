import { useEffect, useState, useSyncExternalStore } from "react";
import { readFile } from "@tauri-apps/plugin-fs";
import { relativePath } from "./format/xml";

const KEY = "choregraphe-share-dir";
const listeners = new Set<() => void>();
export const getShareDir = () => localStorage.getItem(KEY);
export function setShareDir(dir: string) {
  localStorage.setItem(KEY, dir);
  for (const value of cache.values())
    void value.then((url) => {
      if (url) URL.revokeObjectURL(url);
    });
  cache.clear();
  listeners.forEach((listener) => listener());
}
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};
export const useShareDir = () => useSyncExternalStore(subscribe, getShareDir);
const cache = new Map<string, Promise<string | null>>();
function iconUrl(dir: string, rel: string): Promise<string | null> {
  let path: string;
  try {
    path = `${dir}/${relativePath(rel)}`;
  } catch {
    return Promise.resolve(null);
  }
  if (!cache.has(path))
    cache.set(
      path,
      readFile(path).then(
        (b) => URL.createObjectURL(new Blob([b], { type: "image/png" })),
        () => null,
      ),
    );
  return cache.get(path)!;
}
export function useIcon(rel: string | undefined): string | null {
  const dir = useShareDir();
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setUrl(null);
    if (rel && dir)
      void iconUrl(dir, rel).then((value) => {
        if (live) setUrl(value);
      });
    return () => {
      live = false;
    };
  }, [rel, dir]);
  return url;
}
