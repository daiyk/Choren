/** One submesh of an install mesh, decoded from `robot_mesh` (choren_robot::ogre::to_bytes). */
export interface MeshPart {
  positions: Float32Array;
  normals: Float32Array;
  indices: Uint32Array;
  rgb: number;
  /** 0 plain, 1 left eye, 2 right eye, 3 chest button: coloured from the LEDs. */
  tag: number;
}

export function decodeMesh(buffer: ArrayBuffer): MeshPart[] {
  const u32 = new Uint32Array(buffer);
  const parts: MeshPart[] = [];
  let at = 1;
  for (let i = 0; i < u32[0]; i++) {
    const [color, vertices, indices] = [u32[at], u32[at + 1], u32[at + 2]];
    at += 3;
    const floats = (n: number) => {
      const f = new Float32Array(buffer, at * 4, n);
      at += n;
      return f;
    };
    const positions = floats(vertices * 3);
    const normals = floats(vertices * 3);
    parts.push({
      positions,
      normals,
      indices: u32.subarray(at, (at += indices)),
      rgb: color & 0xffffff,
      tag: color >>> 24,
    });
  }
  return parts;
}
