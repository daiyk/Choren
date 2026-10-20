import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode, type MouseEvent, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";

export interface MenuAction {
  label: string;
  run: () => unknown;
  disabled?: boolean;
  reason?: string;
  checked?: boolean;
}
export type MenuEvent = MouseEvent<Element> | KeyboardEvent<Element>;
const nativeContent = "input,textarea,select,[contenteditable=true],.cm-editor,audio,video";
export const copyText = (text: string) => navigator.clipboard.writeText(text);

/** Menus use a viewport overlay so dock clipping cannot hide their actions. */
export function ContextMenu({ x, y, label, boundary, origin, onClose, children }: {
  x: number; y: number; label: string; boundary?: HTMLElement | null; origin?: Element;
  onClose: (restoreFocus?: boolean) => void; children: ReactNode;
}) {
  const menu = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = menu.current!;
    const bounds = () => boundary?.getBoundingClientRect() ?? { left: 0, top: 0, right: innerWidth, bottom: innerHeight, width: innerWidth, height: innerHeight };
    const place = () => {
      const b = bounds();
      element.style.minWidth = `${Math.min(180, Math.max(0, b.width - 8))}px`;
      element.style.maxWidth = `${Math.max(0, b.width - 8)}px`;
      element.style.maxHeight = `${Math.max(0, b.height - 8)}px`;
      element.style.left = `${Math.max(b.left + 4, Math.min(x, b.right - element.offsetWidth - 4))}px`;
      element.style.top = `${Math.max(b.top + 4, Math.min(y, b.bottom - element.offsetHeight - 4))}px`;
    };
    place();
    (element.querySelector<HTMLElement>("button:not(:disabled)") ?? element).focus({ preventScroll: true });
    const observer = new ResizeObserver(place);
    observer.observe(element);
    const outside = (event: Event) => {
      if (!element.contains(event.target as Node)) onClose();
    };
    const close = () => onClose();
    const otherMenu = (event: Event) => { if ((event as CustomEvent).detail !== element) close(); };
    window.dispatchEvent(new CustomEvent("studio-context-open", { detail: element }));
    window.addEventListener("studio-context-open", otherMenu);
    document.addEventListener("pointerdown", outside, true);
    const scrollPositions = new Map<Element, [number, number]>();
    const scrolled = (event: Event) => {
      const target = event.target === document ? document.scrollingElement : event.target as Element;
      const previous = target && scrollPositions.get(target);
      if (previous && target!.scrollLeft === previous[0] && target!.scrollTop === previous[1]) return;
      outside(event);
    };
    // Ignore scroll notifications queued by focusing the trigger before opening.
    const scrollFrame = requestAnimationFrame(() => {
      for (let parent = origin ?? boundary ?? document.activeElement; parent; parent = parent.parentElement)
        scrollPositions.set(parent, [parent.scrollLeft, parent.scrollTop]);
      if (document.scrollingElement) scrollPositions.set(document.scrollingElement, [document.scrollingElement.scrollLeft, document.scrollingElement.scrollTop]);
      document.addEventListener("scroll", scrolled, true);
    });
    window.addEventListener("resize", close);
    window.addEventListener("studio-context-dismiss", close);
    window.addEventListener("blur", close);
    return () => {
      observer.disconnect();
      document.removeEventListener("pointerdown", outside, true);
      cancelAnimationFrame(scrollFrame);
      document.removeEventListener("scroll", scrolled, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("studio-context-dismiss", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("studio-context-open", otherMenu);
    };
  }, [x, y, boundary, origin, onClose]);
  return createPortal(<div ref={menu} className="graph-context-menu studio-context-menu" role="menu" aria-label={label}
    tabIndex={-1} style={{ left: x, top: y }}
    onPointerDown={event => event.stopPropagation()}
    onMouseDown={event => event.stopPropagation()}
    onClick={event => event.stopPropagation()}
    onContextMenu={event => { event.preventDefault(); event.stopPropagation(); }}
    onKeyDown={event => {
      const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
      if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
        event.preventDefault(); event.stopPropagation();
        const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const index = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
          : (at + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
        buttons[index]?.focus();
      }
      if (event.key === "Escape" || event.key === "Tab") {
        if (event.key === "Escape") event.preventDefault();
        event.stopPropagation(); onClose(true);
      }
    }}>{children}</div>, document.body);
}

/** Identity changes invalidate captured callbacks, including pending asynchronous operations. */
export function useContextMenu(identity: readonly unknown[], onError?: (message: string) => void) {
  const current = useRef(identity);
  current.current = identity;
  const [state, setState] = useState<{ x: number; y: number; actions: MenuAction[]; trigger: Element; identity: readonly unknown[] }>();
  const stateRef = useRef(state);
  stateRef.current = state;
  const close = useCallback((restore = false) => {
    const trigger = stateRef.current?.trigger;
    if (restore && trigger?.isConnected) (trigger as HTMLElement).focus({preventScroll: true});
    setState(undefined);
  }, []);
  const valid = (snapshot: readonly unknown[]) => snapshot.length === current.current.length && snapshot.every((v, i) => Object.is(v, current.current[i]));
  useEffect(() => { if (state && !valid(state.identity)) close(); });
  useEffect(() => {
    if (!state) return;
    const observer = new MutationObserver(() => {if (!state.trigger.isConnected) close();});
    observer.observe(document.body, {childList: true, subtree: true});
    return () => observer.disconnect();
  }, [state, close]);
  const open = (event: MenuEvent, actions: MenuAction[]) => {
    if ((event.target as Element).closest(nativeContent) || !actions.length) return;
    event.preventDefault(); event.stopPropagation();
    const rect = event.currentTarget.getBoundingClientRect();
    setState({ x: "clientX" in event ? event.clientX : rect.left, y: "clientY" in event ? event.clientY : rect.bottom,
      actions, trigger: event.currentTarget, identity: [...identity] });
  };
  const trigger = (actions: (event: MenuEvent) => MenuAction[] | undefined) => ({
    onContextMenu: (event: MouseEvent<Element>) => {
      if ((event.target as Element).closest(nativeContent)) return;
      const items = actions(event); if (items) open(event, items);
    },
    onKeyDown: (event: KeyboardEvent<Element>) => {
      if (event.key !== "ContextMenu" && !(event.key === "F10" && event.shiftKey)) return;
      if ((event.target as Element).closest(nativeContent)) return;
      const items = actions(event); if (items) open(event, items);
    },
  });
  const element = state && valid(state.identity) ? <ContextMenu origin={state.trigger} x={state.x} y={state.y} label="Context actions" onClose={close}>
    {state.actions.map((action, i) => <button key={i} role={action.checked === undefined ? "menuitem" : "menuitemcheckbox"}
      aria-checked={action.checked} disabled={action.disabled} title={action.disabled ? action.reason ?? "Unavailable in the current state." : undefined}
      onClick={() => {
        if (!state.trigger.isConnected || !valid(state.identity) || action.disabled) { close(); return; }
        close(true);
        try { void Promise.resolve(action.run()).catch(error => onError?.(String(error))); }
        catch (error) { onError?.(String(error)); }
      }}>{action.checked === undefined ? "" : action.checked ? "✓ " : "　"}{action.label}</button>)}
  </ContextMenu> : null;
  return { open, close, trigger, element };
}
