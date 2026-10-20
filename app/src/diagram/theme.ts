// Port type colors (Dynamic, Bang, Number, String); surfaces follow the workspace theme.
export const theme = {
  canvas: "var(--canvas)",
  line: "var(--node-line)",
  halo: "var(--accent)",
  linkSelected: "var(--accent)",
  ioType: ["#8a94a3", "var(--link)", "#e0a43a", "#3d8fd6"],
};
export const typeColor = (t: number) => theme.ioType[t] ?? theme.ioType[1];
