import { useLayoutEffect, useRef, type ReactNode } from "react";

/** Shared positioning and keyboard behavior for diagram and port menus. */
export function ContextMenu({ x, y, label, onClose, children }: {
  x: number;
  y: number;
  label: string;
  onClose: (restoreFocus?: boolean) => void;
  children: ReactNode;
}) {
  const menu = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = menu.current!;
    const parent = element.parentElement!;
    const place = () => {
      element.style.left = `${Math.max(4, Math.min(x, parent.clientWidth - element.offsetWidth - 4))}px`;
      element.style.top = `${Math.max(4, Math.min(y, parent.clientHeight - element.offsetHeight - 4))}px`;
    };
    const observer = new ResizeObserver(place);
    observer.observe(element);
    observer.observe(parent);
    place();
    (element.querySelector<HTMLElement>("button:not(:disabled)") ?? element).focus({ preventScroll: true });
    const outside = (event: PointerEvent) => {
      if (!element.contains(event.target as Node)) onClose();
    };
    document.addEventListener("pointerdown", outside, true);
    return () => {
      observer.disconnect();
      document.removeEventListener("pointerdown", outside, true);
    };
  }, [x, y, onClose]);
  return <div ref={menu} className="graph-context-menu" role="menu" aria-label={label}
    tabIndex={-1} style={{ left: x, top: y }}
    onContextMenu={event => { event.preventDefault(); event.stopPropagation(); }}
    onKeyDown={event => {
      const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
      if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
        event.preventDefault();
        event.stopPropagation();
        const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const index = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
          : (at + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
        buttons[index]?.focus();
      }
      if (event.key === "Escape" || event.key === "Tab") {
        if (event.key === "Escape") event.preventDefault();
        event.stopPropagation();
        onClose(true);
      }
    }}>{children}</div>;
}
