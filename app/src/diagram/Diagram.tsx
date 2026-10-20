import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  Background,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  getNodesBounds,
  getViewportForBounds,
  useReactFlow,
  useStoreApi,
  useUpdateNodeInternals,
  useViewport,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import { Focus, Map, Minus, Plus, Scan } from "lucide-react";
import type { Box, Diagram as DiagramModel, Port } from "../format/types";
import type { Selection, Viewport } from "../workspace/model";
import { paramPort, resolveLink } from "../format/xar";
import {
  Anchor,
  BoxNode,
  INPUT_GLYPH,
  OUTPUT_GLYPH,
  PortGlyph,
  type BoxNodeType,
} from "./BoxNode";
import { LinkEdge, type LinkEdgeType } from "./LinkEdge";
import { PITCH, RAIL_W, boxSize, classicBoxSize, inputAnchor, outputAnchor } from "./geometry";
import { ClassicBoxNode } from "./ClassicBoxNode";
import type { GraphStyle } from "../workspace/storage";
import { typeColor } from "./theme";
import { boxCategory, categoryColor } from "./appearance";
import { LIBRARY_BOX_MIME } from "../platform";
import type { EditCommand } from "../workspace/authoring";
import { Modal } from "../workspace/Modal";
import { Inspector } from "../Inspector";

const LEFT = "rail-left",
  RIGHT = "rail-right";
type RailNodeType = Node<
  { ports: Port[]; left: boolean; height: number; zoom: number; onActivate?: (port: Port) => void },
  "rail"
>;
const RailNode = memo(function RailNode({ data, isConnectable }: NodeProps<RailNodeType>) {
  const slot = (i: number) => ({ x: 3, y: 16 + PITCH * i });
  const update = useUpdateNodeInternals();
  useEffect(() => {
    update(data.left ? LEFT : RIGHT);
  }, [data.zoom, data.height, data.left, data.ports, update]);
  return (
    <div style={{ width: RAIL_W / data.zoom, height: data.height / data.zoom }}>
      <svg
        width={RAIL_W / data.zoom}
        height={data.height / data.zoom}
        viewBox={`0 0 ${RAIL_W} ${data.height}`}
        overflow="visible"
        className="rail-surface"
      >
        <rect
          width={RAIL_W}
          height={data.height}
          fill="var(--surface)"
          stroke="var(--border)"
        />
        {data.ports.map((p, i) => (
          <PortGlyph
            key={p.id}
            slot={slot(i)}
            port={p}
            rightTab={data.left}
            glyph={(data.left ? INPUT_GLYPH : OUTPUT_GLYPH)[p.nature]}
          />
        ))}
      </svg>
      {data.ports.map((p, i) => {
        const anchor = (data.left ? outputAnchor : inputAnchor)(slot(i));
        return (
          <Anchor
            key={p.id}
            id={`${data.left ? "o" : "i"}${p.id}`}
            at={{ x: anchor.x / data.zoom, y: anchor.y / data.zoom }}
            source={data.left}
            label={p.name}
            tooltip={p.tooltip}
            isConnectable={isConnectable}
            zoom={data.zoom}
            onActivate={data.left && data.onActivate ? () => data.onActivate?.(p) : undefined}
          />
        );
      })}
    </div>
  );
});
const StyledBoxNode = memo(function StyledBoxNode(props: NodeProps<BoxNodeType>) {
  return props.data.graphStyle === "classic"
    ? <ClassicBoxNode {...props} />
    : <BoxNode {...props} />;
});
const nodeTypes = { box: StyledBoxNode, rail: RailNode };
const edgeTypes = { link: LinkEdge };

export function buildGraph(
  diagram: DiagramModel,
  owner: Box,
  editable = false,
  graphStyle: GraphStyle = "choren",
) {
  const links = diagram.links.flatMap((l, i) => {
    const r = resolveLink(l, diagram, owner);
    return r ? [{ l, r, i }] : [];
  });
  const boxes: BoxNodeType[] = diagram.boxes.map((box) => {
    const params = box.params
      .filter((p) =>
        links.some(({ l }) => l.inOwner === box.id && l.inPort === p.id),
      )
      .map(paramPort);
    const nIn = box.inputs.filter((p) => !p.inner).length + params.length;
    const nOut = box.outputs.filter((p) => !p.inner).length;
    const size = graphStyle === "classic"
      ? classicBoxSize(nIn, nOut, box.plugin === "textedit_plugin")
      : boxSize(nIn, nOut, box.plugin === "textedit_plugin", boxCategory(box) === "subgraph");
    return {
      id: String(box.id),
      type: "box",
      position: { x: box.x, y: box.y },
      data: { box, params, graphStyle },
      width: size.w,
      height: size.h,
      // Controlled nodes must retain measurements or React Flow discards their handle bounds.
      measured: { width: size.w, height: size.h },
      ariaLabel: box.name,
      draggable: editable,
      connectable: editable,
      deletable: false,
    };
  });
  const edges: LinkEdgeType[] = links.map(({ l, r, i }) => ({
    id: `l${i}`,
    type: "link",
    source: l.outOwner === 0 ? LEFT : String(l.outOwner),
    sourceHandle: `o${l.outPort}`,
    target: l.inOwner === 0 ? RIGHT : String(l.inOwner),
    targetHandle: `i${l.inPort}`,
    data: { color: typeColor(r.from.port.type) },
    deletable: false,
    reconnectable: false,
    ariaLabel: `${r.from.port.name} to ${r.to.port.name}`,
  }));
  return { boxes, edges };
}
export interface GraphControls {
  fit: () => void;
  fitSelection: () => void;
  zoomIn: () => void;
  zoomOut: () => void;
}
interface Props {
  graphStyle?: GraphStyle;
  diagram: DiagramModel;
  owner: Box;
  selection: Selection;
  viewport?: Viewport;
  onViewport: (v: Viewport) => void;
  onSelect: (selection: Selection) => void;
  onOpen: (box: Box) => void;
  onParameters?: (box: Box) => void;
  onActivateInput?: (box: Box, port: Port) => void;
  onInsertLibrary?: (path: string, position: { x: number; y: number }) => void;
  onControls?: (controls: GraphControls | undefined) => void;
  /** Present when the project can be edited (desktop app with a backend session). */
  onEdit?: (label: string, commands: EditCommand[]) => Promise<unknown>;
  /** Runs a workspace command such as "delete", "copy" or "paste". */
  onCommand?: (id: string) => void;
  /** Box ids the running behavior just stimulated. */
  glowing?: Set<string>;
}
function Graph({
  graphStyle = "choren",
  diagram,
  owner,
  selection,
  viewport: savedViewport,
  onViewport,
  onSelect,
  onOpen,
  onParameters,
  onActivateInput,
  onInsertLibrary,
  onControls,
  onEdit,
  onCommand,
  glowing,
}: Props) {
  const editable = !!onEdit && diagram.node !== undefined;
  const [contextMenu, setContextMenu] = useState<{x: number; y: number; box?: Box}>();
  const [portMenu, setPortMenu] = useState<{x: number; y: number; box: Box; port: Port; source: boolean; ownerId: number}>();
  const [parameterLink, setParameterLink] = useState<{box: Box; outOwner: number; outPort: number}>();
  const [editingPorts, setEditingPorts] = useState<number>();
  const portBox = editingPorts === 0 ? owner : diagram.boxes.find(b => b.id === editingPorts);
  const root = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 800, height: 600 });
  const [minimap, setMinimap] = useState(false);
  const viewport = useViewport();
  const flow = useReactFlow();
  const store = useStoreApi();
  const cancelClickConnection = () => store.setState({ connectionClickStartHandle: null });
  useEffect(() => {
    store.setState({ connectionClickStartHandle: null });
  }, [graphStyle, store]);
  const fitted = useRef(false);
  // React Flow can emit node and edge changes in one event before React renders.
  const selectionRef = useRef(selection);
  selectionRef.current = selection;
  const select = (next: Selection) => {
    selectionRef.current = next;
    onSelect(next);
  };
  const built = useMemo(
    () => buildGraph(diagram, owner, editable, graphStyle),
    [diagram, owner, editable, graphStyle],
  );
  // Keep local positions through the save round trip so drops never flash the old document.
  const [dragging, setDragging] = useState<
    Record<string, { x: number; y: number }>
  >({});
  useEffect(() => {
    const observer = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width > 0 && entry.contentRect.height > 0)
        setSize({
          width: entry.contentRect.width,
          height: entry.contentRect.height,
        });
    });
    if (root.current) observer.observe(root.current);
    return () => observer.disconnect();
  }, []);
  const controls = useMemo<GraphControls>(
    () => ({
      fit: () => {
        void flow.fitView({
          nodes: built.boxes,
          padding: 0.18,
          maxZoom: 1.25,
          duration: 180,
        });
      },
      fitSelection: () => {
        if (selection.nodes.length)
          void flow.fitView({
            nodes: built.boxes.filter((b) => selection.nodes.includes(b.id)),
            padding: 0.4,
            maxZoom: 1.6,
            duration: 180,
          });
      },
      zoomIn: () => {
        void flow.zoomIn({ duration: 120 });
      },
      zoomOut: () => {
        void flow.zoomOut({ duration: 120 });
      },
    }),
    [flow, built.boxes, selection.nodes],
  );
  useEffect(() => {
    onControls?.(controls);
    return () => onControls?.(undefined);
  }, [controls, onControls]);
  useEffect(() => {
    if (!flow.viewportInitialized || fitted.current) return;
    const rect = root.current?.getBoundingClientRect();
    if (!rect?.width || !rect.height) return;
    fitted.current = true;
    // Node sizes are known from the format. Fitting their bounds avoids waiting for offscreen rails.
    const initial =
      savedViewport ??
      (built.boxes.length
        ? getViewportForBounds(
            getNodesBounds(built.boxes),
            rect.width,
            rect.height,
            0.1,
            1.25,
            0.18,
          )
        : { x: 0, y: 0, zoom: 1 });
    void flow.setViewport(initial);
  }, [flow, flow.viewportInitialized, savedViewport, built.boxes, size]);
  const rails: RailNodeType[] = [true, false].map((left) => ({
    id: left ? LEFT : RIGHT,
    type: "rail",
    position: {
      x: ((left ? 0 : size.width - RAIL_W) - viewport.x) / viewport.zoom,
      y: -viewport.y / viewport.zoom,
    },
    selectable: false,
    draggable: false,
    deletable: false,
    focusable: false,
    zIndex: 2,
    width: RAIL_W / viewport.zoom,
    height: size.height / viewport.zoom,
    measured: {
      width: RAIL_W / viewport.zoom,
      height: size.height / viewport.zoom,
    },
    data: {
      onActivate: onActivateInput ? (port) => {
        cancelClickConnection();
        onActivateInput(owner, port);
      } : undefined,
      ports: left ? owner.inputs : owner.outputs,
      left,
      height: size.height,
      zoom: viewport.zoom,
    },
  }));
  const nodes: Node[] = [
    ...built.boxes.map((b) => ({
      ...b,
      data: {
        ...b.data,
        onActivate: onActivateInput ? (port: Port) => {
          cancelClickConnection();
          onActivateInput(b.data.box, port);
        } : undefined,
        onParameters: () => {
          select({ nodes: [b.id], edges: [] });
          onParameters?.(b.data.box);
        },
      },
      position: dragging[b.id] ?? b.position,
      selected: selection.nodes.includes(b.id),
      className: glowing?.has(b.id) ? "stimulated" : undefined,
    })),
    ...rails,
  ];
  const edges = built.edges.map((e) => ({
    ...e,
    selected: selection.edges.includes(e.id),
  }));
  return (
    <div
      className={editable ? "graph editable" : "graph"}
      ref={root}
      data-testid="graph"
      data-graph-style={graphStyle}
      onContextMenuCapture={e => {
        const handle = (e.target as Element).closest<HTMLElement>(".react-flow__handle");
        if (!handle) return;
        e.preventDefault(); e.stopPropagation(); cancelClickConnection(); setContextMenu(undefined);
        const nodeId = handle.dataset.nodeid;
        const source = handle.dataset.handleid?.startsWith("o") ?? false;
        const id = Number(handle.dataset.handleid?.slice(1));
        const box = nodeId === LEFT || nodeId === RIGHT ? owner : diagram.boxes.find(b => String(b.id) === nodeId);
        if (!box) return;
        const rail = nodeId === LEFT || nodeId === RIGHT;
        const port = (rail ? source ? box.inputs : box.outputs : source ? box.outputs : [...box.inputs, ...box.params.map(paramPort)]).find(p => p.id === id);
        if (!port) return;
        select({nodes: rail ? [] : [String(box.id)], edges: []});
        const rect = root.current!.getBoundingClientRect();
        setPortMenu({x: Math.max(0, Math.min(e.clientX - rect.left, rect.width - 200)), y: Math.max(0, Math.min(e.clientY - rect.top, rect.height - 180)), box, port, source, ownerId: rail ? 0 : box.id});
      }}
      onDragOver={(event) => {
        if (diagram.node === undefined || !onInsertLibrary || !event.dataTransfer.types.includes(LIBRARY_BOX_MIME)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = "copy";
      }}
      onDrop={(event) => {
        if (diagram.node === undefined || !onInsertLibrary || !event.dataTransfer.types.includes(LIBRARY_BOX_MIME)) return;
        event.preventDefault();
        event.stopPropagation();
        const path = event.dataTransfer.getData(LIBRARY_BOX_MIME);
        if (path) onInsertLibrary(path, flow.screenToFlowPosition({ x: event.clientX, y: event.clientY }));
      }}
      onKeyDown={(e) => {
        if (
          (e.target as HTMLElement).closest(
            "input,textarea,[contenteditable=true],dialog",
          )
        )
          return;
        if (e.key === "Escape") { cancelClickConnection(); setContextMenu(undefined); setPortMenu(undefined); }
        if ((e.key === "F10" && e.shiftKey) || e.key === "ContextMenu") {
          const target = (e.target as Element).closest<HTMLElement>(".react-flow__handle");
          if (target) {e.preventDefault(); const rect = target.getBoundingClientRect(); target.dispatchEvent(new MouseEvent("contextmenu", {bubbles: true, clientX: rect.left, clientY: rect.bottom}));}
        }
        if (e.defaultPrevented || (e.target as Element).closest('[role="menu"]')) return;
        if (e.key === "Backspace" || e.key === "Delete") {
          e.preventDefault();
          if (editable) onCommand?.("delete");
        }
        if (editable && (e.metaKey || e.ctrlKey) && !e.shiftKey) {
          const key = e.key.toLowerCase();
          if (key === "c" || key === "v" || key === "x") {
            e.preventDefault();
            onCommand?.(key === "c" ? "copy" : key === "x" ? "cut" : "paste");
          }
        }
        if (e.key === "Enter" && selection.nodes.length === 1) {
          const b = diagram.boxes.find(
            (b) => String(b.id) === selection.nodes[0],
          );
          if (b) onOpen(b);
        }
      }}
    >
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        ariaLabelConfig={{
          "node.a11yDescription.default": `Press Space to select a box, Enter to open it, or Escape to clear selection. ${editable ? "Delete removes it." : "This graph is read-only."}`,
          "node.a11yDescription.keyboardDisabled": `Press Space to select a box, Enter to open it, or Escape to clear selection. ${editable ? "Delete removes it." : "This graph is read-only."}`,
          "edge.a11yDescription.default": `Press Enter or Space to select a connection, or Escape to clear selection. ${editable ? "Delete removes it." : "This graph is read-only."}`,
        }}
        nodesDraggable={editable}
        nodesConnectable={editable}
        onNodeDrag={(_, __, nodes) =>
          setDragging((d) => ({
            ...d,
            ...Object.fromEntries(nodes.map((n) => [n.id, n.position])),
          }))
        }
        onNodeDragStop={async (_, __, nodes) => {
          const dropped = Object.fromEntries(nodes.map((n) => [n.id, {
            x: Math.round(n.position.x), y: Math.round(n.position.y),
          }]));
          setDragging((current) => ({ ...current, ...dropped }));
          const moves = nodes.flatMap((n) => {
            const box = diagram.boxes.find((b) => String(b.id) === n.id);
            return box?.node !== undefined
              ? [
                  [
                    box.node,
                    Math.round(n.position.x),
                    Math.round(n.position.y),
                  ] as [number, number, number],
                ]
              : [];
          });
          try {
            if (moves.length)
              await onEdit?.("Drag selection in diagram", [{ type: "moveBoxes", moves }]);
          } finally {
            setDragging((current) => {
              const next = { ...current };
              // A newer drag may already be in progress while this save finishes.
              for (const id of Object.keys(dropped))
                if (next[id] === dropped[id]) delete next[id];
              return next;
            });
          }
        }}
        onConnect={(c) => {
          if (!editable || !c.sourceHandle || !c.targetHandle) return;
          onEdit?.("Add a link", [
            {
              type: "addLink",
              diagram: diagram.node!,
              outOwner: c.source === LEFT ? 0 : Number(c.source),
              outPort: Number(c.sourceHandle.slice(1)),
              inOwner: c.target === RIGHT ? 0 : Number(c.target),
              inPort: Number(c.targetHandle.slice(1)),
            },
          ]);
        }}
        onConnectEnd={(event, connection) => {
          if (!editable || connection.isValid || connection.fromHandle?.type !== "source" || !connection.fromHandle.id) return;
          const point = "changedTouches" in event ? event.changedTouches[0] : event;
          if (!point) return;
          const node = document.elementFromPoint(point.clientX, point.clientY)?.closest<HTMLElement>(".react-flow__node");
          const box = diagram.boxes.find(b => String(b.id) === node?.dataset.id);
          if (box?.params.length) {
            cancelClickConnection();
            setParameterLink({box, outOwner: connection.fromNode?.id === LEFT ? 0 : Number(connection.fromNode?.id), outPort: Number(connection.fromHandle.id.slice(1))});
          }
        }}
        edgesReconnectable={false}
        deleteKeyCode={null}
        zoomOnDoubleClick={false}
        minZoom={0.1}
        maxZoom={4}
        defaultViewport={savedViewport ?? { x: 0, y: 0, zoom: 1 }}
        onlyRenderVisibleElements
        onMoveEnd={(_, v) => onViewport(v)}
        onNodesChange={(changes) => {
          const selected = new Set(selectionRef.current.nodes);
          for (const change of changes)
            if (change.type === "select" && !change.id.startsWith("rail-")) {
              if (change.selected) selected.add(change.id);
              else selected.delete(change.id);
            }
          if (changes.some((c) => c.type === "select"))
            select({ ...selectionRef.current, nodes: [...selected] });
        }}
        onEdgesChange={(changes) => {
          const selected = new Set(selectionRef.current.edges);
          for (const change of changes)
            if (change.type === "select") {
              if (change.selected) selected.add(change.id);
              else selected.delete(change.id);
            }
          if (changes.some((c) => c.type === "select"))
            select({ ...selectionRef.current, edges: [...selected] });
        }}
        onPaneContextMenu={e => {
          setPortMenu(undefined);
          e.preventDefault(); const rect = root.current!.getBoundingClientRect();
          setContextMenu({x: Math.max(0, Math.min(e.clientX - rect.left, rect.width - 190)), y: Math.max(0, Math.min(e.clientY - rect.top, rect.height - 250))});
        }}
        onNodeContextMenu={(e, n) => {
          setPortMenu(undefined);
          e.preventDefault();
          if (n.type !== "box") return;
          if (!selection.nodes.includes(n.id)) select({nodes: [n.id], edges: []});
          const rect = root.current!.getBoundingClientRect();
          setContextMenu({x: Math.max(0, Math.min(e.clientX - rect.left, rect.width - 190)), y: Math.max(0, Math.min(e.clientY - rect.top, rect.height - 250)), box: (n as BoxNodeType).data.box});
        }}
        onNodeDoubleClick={(_, n) =>
          n.type === "box" && onOpen((n as BoxNodeType).data.box)
        }
        onPaneClick={() => {
          setContextMenu(undefined);
          setPortMenu(undefined);
          cancelClickConnection();
          select({ nodes: [], edges: [] });
        }}
        multiSelectionKeyCode={["Meta", "Control", "Shift"]}
        selectionOnDrag
        panOnDrag={[1, 2]}
        panActivationKeyCode="Space"
      >
        <Background color="var(--grid-dot)" gap={20} size={1} />
        {minimap && (
          <MiniMap
            nodeColor={(node) => node.type !== "box" ? "transparent" : graphStyle === "classic" ? "var(--accent)" : categoryColor(boxCategory((node as BoxNodeType).data.box))}
            nodeStrokeWidth={0}
            maskColor="var(--minimap-mask)"
            pannable
            zoomable
          />
        )}
      </ReactFlow>
      {portMenu && <div className="graph-context-menu" role="menu" aria-label={`Port ${portMenu.port.name}`} style={{left: portMenu.x, top: portMenu.y}}
        onKeyDown={e => {
          const buttons = [...e.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {e.preventDefault(); const at = buttons.indexOf(document.activeElement as HTMLButtonElement); buttons[(at + (e.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length]?.focus();}
        }}>
        <button role="menuitem" autoFocus onClick={() => {setEditingPorts(portMenu.ownerId); setPortMenu(undefined);}}>Edit {portMenu.port.name}…</button>
        {onActivateInput && (portMenu.ownerId === 0 ? portMenu.source : !portMenu.source && portMenu.box.inputs.some(p => p.id === portMenu.port.id)) && <button role="menuitem" onClick={() => {onActivateInput(portMenu.box, portMenu.port); setPortMenu(undefined);}}>Activate input</button>}
        <button role="menuitem" disabled={!editable || !diagram.links.some(l => portMenu.source ? l.outOwner === portMenu.ownerId && l.outPort === portMenu.port.id : l.inOwner === portMenu.ownerId && l.inPort === portMenu.port.id)} onClick={() => {
          const links = diagram.links.filter(l => portMenu.source ? l.outOwner === portMenu.ownerId && l.outPort === portMenu.port.id : l.inOwner === portMenu.ownerId && l.inPort === portMenu.port.id);
          void onEdit?.("Disconnect port", [{type: "remove", nodes: links.flatMap(l => l.node === undefined ? [] : [l.node])}]); setPortMenu(undefined);
        }}>Disconnect links</button>
        <button role="menuitem" disabled={!editable || portMenu.port.node === undefined} onClick={() => {void onEdit?.("Delete port", [{type: "remove", nodes: [portMenu.port.node!]}]); setPortMenu(undefined);}}>Delete port</button>
      </div>}
      {portBox && editingPorts !== undefined && <Modal title={`Port properties: ${portBox.name}`} wide onClose={() => setEditingPorts(undefined)}>
        <Inspector boxes={[{...portBox, id: 1}]} onEdit={onEdit} onScript={() => {setEditingPorts(undefined); onOpen(portBox);}} />
      </Modal>}
      {parameterLink && <Modal title={`Connect to parameter of ${parameterLink.box.name}`} onClose={() => setParameterLink(undefined)}>
        <div className="settings-content"><p>Choose the parameter that receives this output.</p>{parameterLink.box.params.map(p => <button key={p.id} onClick={() => {
          void onEdit?.("Connect parameter", [{type: "addLink", diagram: diagram.node!, outOwner: parameterLink.outOwner, outPort: parameterLink.outPort, inOwner: parameterLink.box.id, inPort: p.id}]); setParameterLink(undefined);
        }}>{p.name}</button>)}</div>
      </Modal>}
      {contextMenu && <div className="graph-context-menu" role="menu" aria-label="Diagram actions" style={{left: contextMenu.x, top: contextMenu.y}}
        onKeyDown={e => {
          const buttons = [...e.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
          if (e.key === "ArrowDown" || e.key === "ArrowUp") { e.preventDefault(); const at = buttons.indexOf(document.activeElement as HTMLButtonElement); buttons[(at + (e.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length]?.focus(); }
        }}>
        {contextMenu.box && <button role="menuitem" autoFocus onClick={() => {onOpen(contextMenu.box!); setContextMenu(undefined);}}>Open {contextMenu.box.name}</button>}
        {[["insert", "Create a new box…"], ["cut", "Cut"], ["copy", "Copy"], ["paste", "Paste"], ["delete", "Delete"], ["convert-to-box", "Convert to box…"]].map(([id,label],i) => <button role="menuitem" key={id} autoFocus={!contextMenu.box && i === 0}
          disabled={!editable || (["cut", "copy", "delete", "convert-to-box"].includes(id) && !selection.nodes.length)}
          onClick={() => {onCommand?.(id); setContextMenu(undefined);}}>{label}</button>)}
        <button role="menuitem" onClick={() => {controls.fit(); setContextMenu(undefined);}}>Fit graph</button>
      </div>}
      {!diagram.boxes.length && (
        <div className="canvas-message">This diagram has no boxes.</div>
      )}
      <div className="graph-controls" role="toolbar" aria-label="Graph view">
        <button
          aria-label="Zoom out"
          title="Zoom out"
          onClick={controls.zoomOut}
        >
          <Minus size={15} />
        </button>
        <select aria-label="Zoom preset" value="" onChange={e => {if (e.target.value === "fit") controls.fit(); else void flow.zoomTo(Number(e.target.value) / 100);}}>
          <option value="">Zoom…</option>{[25,50,75,100,125,150,200].map(v => <option key={v} value={v}>{v}%</option>)}<option value="fit">Fit graph</option>
        </select>
        <output aria-label="Zoom">{Math.round(viewport.zoom * 100)}%</output>
        <button aria-label="Zoom in" title="Zoom in" onClick={controls.zoomIn}>
          <Plus size={15} />
        </button>
        <span className="separator" />
        <button aria-label="Fit graph" title="Fit graph" onClick={controls.fit}>
          <Scan size={15} />
        </button>
        <button
          aria-label="Fit selection"
          title="Fit selection"
          disabled={!selection.nodes.length}
          onClick={controls.fitSelection}
        >
          <Focus size={15} />
        </button>
        <button
          aria-label="Toggle minimap"
          title="Toggle minimap"
          aria-pressed={minimap}
          onClick={() => setMinimap(!minimap)}
        >
          <Map size={15} />
        </button>
      </div>
      <span className="canvas-hint">
        {editable
          ? "Drag boxes to move · Click or drag port to port to link · Escape cancels · Delete removes"
          : "Space + drag to pan · Double-click to open"}
      </span>
    </div>
  );
}
export function Diagram(props: Props) {
  return (
    <ReactFlowProvider>
      <Graph {...props} />
    </ReactFlowProvider>
  );
}
