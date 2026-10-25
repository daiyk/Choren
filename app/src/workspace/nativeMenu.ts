import {
  Menu,
  MenuItem,
  PredefinedMenuItem,
  Submenu,
} from "@tauri-apps/api/menu";
import type { Command } from "./registry";
import { mac, native } from "../platform";

export interface NativeMenuHandle {
  dispose: () => void;
  update: (commands: Command[]) => Promise<void>;
}
export async function installNativeMenu(
  commands: Command[],
  execute: (id: string) => void,
): Promise<NativeMenuHandle> {
  if (!native) return { dispose: () => {}, update: async () => {} };
  const commandItems = new Map<string, MenuItem>();
  const groups = ["File", "Edit", "View", "Connection", "Help"] as const;
  const items: Submenu[] = [];
  if (mac)
    items.push(
      await Submenu.new({
        text: "Choren",
        items: [
          // Not the predefined About: the shared dialog looks the same on every platform.
          await MenuItem.new({
            text: "About Choren",
            action: () => execute("about"),
          }),
          await MenuItem.new({
            text: "Settings…",
            action: () => execute("settings"),
            accelerator: "CmdOrCtrl+,",
          }),
          await PredefinedMenuItem.new({ item: "Separator" }),
          // Not the predefined Quit: that terminates without the unsaved-changes guard.
          await MenuItem.new({
            text: "Quit Choren",
            action: () => execute("quit"),
            accelerator: "CmdOrCtrl+Q",
          }),
        ],
      }),
    );
  for (const group of groups) {
    const entries = await Promise.all(
      commands
        .filter(
          (c) =>
            c.group === group &&
            (c.enabled || !c.id.startsWith("panel.")) &&
            !(mac && (c.id === "quit" || c.id === "about")), // macOS: these live in the app menu
        )
        .map(async (command) => {
          const item = await MenuItem.new({
            id: command.id,
            text: command.title,
            enabled: command.enabled,
            accelerator: command.shortcut,
            action: () => execute(command.id),
          });
          commandItems.set(command.id, item);
          return item;
        }),
    );
    if (group === "Edit") {
      const nativeEdit = await Promise.all(
        (["Undo", "Redo", "Cut", "Copy", "Paste", "SelectAll"] as const).map(
          (item) => PredefinedMenuItem.new({ item }),
        ),
      );
      items.push(
        await Submenu.new({
          text: group,
          items: [
            ...nativeEdit,
            await PredefinedMenuItem.new({ item: "Separator" }),
            ...entries,
          ],
        }),
      );
    } else items.push(await Submenu.new({ text: group, items: entries }));
  }
  const menu = await Menu.new({ items });
  await menu.setAsAppMenu();
  return {
    dispose: () => {
      void menu.close();
    },
    update: async (current) => {
      await Promise.all(
        current.map((command) =>
          commandItems.get(command.id)?.setEnabled(command.enabled),
        ),
      );
    },
  };
}
