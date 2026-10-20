import { BaseEdge, type Edge, type EdgeProps } from "@xyflow/react";
import { buildLink } from "./geometry";
import { theme } from "./theme";

export type LinkEdgeType = Edge<{ color: string }, "link">;

/** A link: 2 px round-cap curve in the output's type color; 3 px accent when selected. */
export function LinkEdge({ id, sourceX, sourceY, targetX, targetY, data, selected }: EdgeProps<LinkEdgeType>) {
  return (
    <BaseEdge
      id={id}
      path={buildLink({ x: sourceX, y: sourceY }, { x: targetX, y: targetY })}
      style={{ stroke: selected ? theme.linkSelected : data?.color, strokeWidth: selected ? 3 : 2, strokeLinecap: "round" }}
    />
  );
}
