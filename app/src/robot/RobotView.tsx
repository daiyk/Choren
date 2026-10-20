import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { native } from "../platform";
import { useShareDir } from "../icons";
import { decodeMesh } from "./mesh";
import { getRobot, robot, type RobotDescriptor } from "./store";

const hex = (rgb: number | undefined, fallback: number) =>
  new THREE.Color(rgb ?? fallback);

/**
 * Kinematic 3-D robot drawn from the descriptor: one pivot per joint. The install's Ogre meshes
 * (read by the backend, authored in their link's frame) replace the primitive bones when the
 * Choregraphe folder is configured; the primitives stay as the fallback.
 * With `onDrag`, relaxed joints (stiffness 0) can be dragged with the mouse.
 */
export function RobotView({
  descriptor,
  onDrag,
  options = {world: true, position: true, motors: true},
}: {
  descriptor: RobotDescriptor;
  options?: {world: boolean; position: boolean; motors: boolean};
  onDrag?: (joint: string, angle: number) => void;
}) {
  const display = useRef(options);
  display.current = options;
  const host = useRef<HTMLDivElement>(null);
  const drag = useRef(onDrag);
  drag.current = onDrag;
  const shareDir = useShareDir();
  // Why the primitive robot is shown instead of the install meshes, if it is.
  const [fallback, setFallback] = useState<string>();
  useEffect(() => {
    const el = host.current!;
    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
    el.appendChild(renderer.domElement);
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(40, 1, 0.01, 50);
    camera.position.set(0.9, 0.55, 0.9);
    const controls = new OrbitControls(camera, renderer.domElement);
    controls.target.set(0, 0.3, 0);
    scene.add(new THREE.HemisphereLight(0xffffff, 0x8899aa, 2.2));
    const sun = new THREE.DirectionalLight(0xffffff, 1.4);
    sun.position.set(1, 2, 1.5);
    scene.add(sun);
    const grid = new THREE.GridHelper(6, 60, 0x3a7bd5, 0x9db8de);
    scene.add(grid);

    const shell = new THREE.MeshStandardMaterial({
      color: 0xf4f4f4,
      roughness: 0.5,
    });
    const joint = new THREE.MeshStandardMaterial({
      color: 0x5a6270,
      roughness: 0.6,
    });
    const body = new THREE.Group(); // world placement (odometry)
    const frame = new THREE.Group(); // robot frame: z up
    frame.rotation.x = -Math.PI / 2;
    const tilt = new THREE.Group(); // torso pitch while changing posture
    body.add(frame);
    frame.add(tilt);
    scene.add(body);
    const torso = new THREE.Mesh(
      new THREE.BoxGeometry(0.075, 0.12, 0.17),
      shell,
    );
    torso.position.z = 0.015;
    tilt.add(torso);
    const chest = new THREE.Mesh(
      new THREE.CircleGeometry(0.012, 20),
      new THREE.MeshBasicMaterial(),
    );
    chest.position.set(0.0385, 0, 0.06);
    chest.rotation.y = Math.PI / 2;
    tilt.add(chest);

    const pivots = new Map<string, THREE.Object3D>();
    const axes: THREE.Vector3[] = [];
    for (const j of descriptor.joints) {
      const parent = j.parent ? pivots.get(j.parent)! : tilt;
      const pivot = new THREE.Object3D();
      pivot.position.set(...j.origin);
      pivot.userData.joint = pivots.size;
      parent.add(pivot);
      pivots.set(j.name, pivot);
      axes.push(new THREE.Vector3(...j.axis));
      const length = Math.hypot(...j.origin);
      if (length > 0.001) {
        // Bone from the parent joint to this one.
        const bone = new THREE.Mesh(
          new THREE.CylinderGeometry(0.018, 0.018, length, 12),
          shell,
        );
        bone.position.set(j.origin[0] / 2, j.origin[1] / 2, j.origin[2] / 2);
        bone.quaternion.setFromUnitVectors(
          new THREE.Vector3(0, 1, 0),
          new THREE.Vector3(...j.origin).normalize(),
        );
        parent.add(bone);
      }
      if (j.axis.some((a) => a !== 0))
        pivot.add(
          new THREE.Mesh(new THREE.SphereGeometry(0.022, 14, 10), joint),
        );
    }
    for (const side of ["L", "R"]) {
      const hand = new THREE.Mesh(
        new THREE.BoxGeometry(0.05, 0.03, 0.02),
        shell,
      );
      hand.position.set(0.03, 0, -0.005);
      pivots.get(`${side}Hand`)?.add(hand);
    }
    const tip = (name: string) => descriptor.tips[name];
    const head = new THREE.Mesh(new THREE.SphereGeometry(0.065, 24, 18), shell);
    const [headLink, headAt] = tip("Head") ?? ["HeadPitch", [0, 0, 0.09]];
    head.position.set(...headAt);
    pivots.get(headLink)?.add(head);
    const eyes = ["LeftFaceLeds", "RightFaceLeds"].map((group, i) => {
      const eye = new THREE.Mesh(
        new THREE.SphereGeometry(0.012, 12, 8),
        new THREE.MeshBasicMaterial(),
      );
      eye.position.set(
        headAt[0] + 0.055,
        i === 0 ? 0.025 : -0.025,
        headAt[2] + 0.01,
      );
      pivots.get(headLink)?.add(eye);
      return { group, eye };
    });
    const feet = ["LFoot", "RFoot"].map((name) => {
      const [link, at] = tip(name) ?? [`${name[0]}AnkleRoll`, [0, 0, -0.045]];
      const foot = new THREE.Mesh(
        new THREE.BoxGeometry(0.16, 0.08, 0.02),
        shell,
      );
      foot.position.set(at[0] + 0.02, at[1], at[2] + 0.01);
      pivots.get(link)?.add(foot);
      return foot;
    });

    // LED-coloured parts and the solids that touch the floor; the meshes replace both.
    let leds: [string, THREE.Material][] = [
      ...eyes.map(
        ({ group, eye }) => [group, eye.material] as [string, THREE.Material],
      ),
      ["ChestLeds", chest.material as THREE.Material],
    ];
    let solids: THREE.Object3D[] = [...feet, head, torso];
    const primitives: THREE.Object3D[] = [];
    body.traverse((o) => {
      if (o instanceof THREE.Mesh) primitives.push(o);
    });
    let alive = true;
    const meshes = Object.entries(descriptor.meshes ?? {});
    setFallback(undefined);
    if (native && shareDir && meshes.length)
      void Promise.all(
        meshes.map(([link, name]) =>
          robot
            .mesh(shareDir, descriptor.id, name)
            .then((b) => [link, decodeMesh(b)] as const),
        ),
      ).then(
        (loaded) => {
          if (!alive) return;
          const nextSolids: THREE.Object3D[] = [];
          const nextLeds: [string, THREE.Material][] = [];
          for (const [link, parts] of loaded) {
            const group = new THREE.Group();
            for (const part of parts) {
              const g = new THREE.BufferGeometry();
              g.setAttribute(
                "position",
                new THREE.BufferAttribute(part.positions, 3),
              );
              g.setAttribute(
                "normal",
                new THREE.BufferAttribute(part.normals, 3),
              );
              g.setIndex(new THREE.BufferAttribute(part.indices, 1));
              const m = new THREE.MeshStandardMaterial({
                color: part.rgb,
                roughness: 0.55,
              });
              const led = ["", "LeftFaceLeds", "RightFaceLeds", "ChestLeds"][
                part.tag
              ];
              if (led) nextLeds.push([led, m]);
              group.add(new THREE.Mesh(g, m));
            }
            (link === "Torso" ? tilt : pivots.get(link))?.add(group);
            nextSolids.push(group);
          }
          for (const p of primitives) p.visible = false;
          solids = nextSolids;
          leds = nextLeds;
          el.dataset.model = "meshes";
        },
        (e) => {
          el.dataset.model = "primitives"; // no install meshes: keep the primitive robot
          if (alive) setFallback(`Simple model: the NAO meshes could not be loaded (${String(e)}). Choose the Choregraphe installation in Settings.`);
        },
      );
    else {
      el.dataset.model = "primitives";
      if (native && meshes.length && !shareDir) setFallback("Simple model: choose the Choregraphe installation in Settings to show the NAO meshes.");
    }

    // Dragging a relaxed joint (teach mode): horizontal mouse motion turns it.
    const ray = new THREE.Raycaster();
    let dragging: { index: number; x: number; start: number } | undefined;
    const pick = (e: PointerEvent) => {
      grid.visible = display.current.world;
      const s = getRobot().state;
      if (!drag.current || !s?.stiffness) return;
      const r = renderer.domElement.getBoundingClientRect();
      ray.setFromCamera(
        new THREE.Vector2(
          ((e.clientX - r.left) / r.width) * 2 - 1,
          -((e.clientY - r.top) / r.height) * 2 + 1,
        ),
        camera,
      );
      const hit = ray.intersectObject(body, true).find((h) => h.object.visible);
      for (
        let o: THREE.Object3D | null = hit?.object ?? null;
        o;
        o = o.parent
      ) {
        const index = o.userData.joint as number | undefined;
        if (index === undefined || axes[index].lengthSq() === 0) continue;
        if ((s.stiffness[index] ?? 1) > 0.01) continue;
        dragging = { index, x: e.clientX, start: s.angles[index] ?? 0 };
        controls.enabled = false;
        renderer.domElement.setPointerCapture(e.pointerId);
        return;
      }
    };
    const move = (e: PointerEvent) => {
      if (!dragging) return;
      const j = descriptor.joints[dragging.index];
      const angle = Math.min(
        j.max,
        Math.max(j.min, dragging.start + (e.clientX - dragging.x) * 0.01),
      );
      drag.current?.(j.name, angle);
    };
    const drop = () => {
      dragging = undefined;
      controls.enabled = true;
    };
    renderer.domElement.addEventListener("pointerdown", pick);
    renderer.domElement.addEventListener("pointermove", move);
    renderer.domElement.addEventListener("pointerup", drop);

    const resize = new ResizeObserver(() => {
      const { width, height } = el.getBoundingClientRect();
      if (!width || !height) return;
      renderer.setSize(width, height);
      camera.aspect = width / height;
      // Keep the horizontal field of view when this dock becomes tall and narrow.
      camera.fov = THREE.MathUtils.radToDeg(
        2 *
          Math.atan(
            Math.tan(THREE.MathUtils.degToRad(20)) / Math.min(1, camera.aspect),
          ),
      );
      camera.updateProjectionMatrix();
    });
    resize.observe(el);
    const box = new THREE.Box3();
    let frameId = 0;
    const draw = () => {
      frameId = requestAnimationFrame(draw);
      const s = getRobot().state;
      if (s) {
        descriptor.joints.forEach((j, i) => {
          const a = s.angles[i];
          if (display.current.motors && a !== undefined && axes[i].lengthSq() > 0)
            pivots.get(j.name)!.quaternion.setFromAxisAngle(axes[i], a);
        });
        tilt.rotation.y = s.torsoPitch ?? 0;
        const [x = 0, y = 0, th = 0] = s.pose ?? [];
        body.position.set(display.current.position ? x : 0, 0, display.current.position ? -y : 0);
        body.rotation.y = display.current.position ? th : 0;
        for (const [group, m] of leds) {
          const c = hex(s.leds?.[group], 0xffffff);
          if (m instanceof THREE.MeshStandardMaterial) m.emissive = c;
          else (m as THREE.MeshBasicMaterial).color = c;
        }
      }
      // Ground the lowest point of the body on the floor (no physics: Choregraphe does the same).
      body.updateMatrixWorld(true);
      box.makeEmpty();
      for (const f of solids) box.expandByObject(f);
      body.position.y -= box.min.y;
      controls.update();
      renderer.render(scene, camera);
    };
    draw();
    return () => {
      alive = false;
      cancelAnimationFrame(frameId);
      resize.disconnect();
      controls.dispose();
      renderer.dispose();
      el.removeChild(renderer.domElement);
    };
  }, [descriptor, shareDir]);
  return (
    <>
      <div
        className="robot-canvas"
        ref={host}
        aria-label="3-D robot view"
        role="img"
      />
      {fallback && <p className="robot-model-note" role="status">{fallback}</p>}
    </>
  );
}
