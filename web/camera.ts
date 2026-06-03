// Orbit + zoom camera (BV10). Skeletal MVP-matrix math for phase 1 — no
// readback, no sim coupling. Produces a column-major 4x4 view-projection
// matrix the renderer will use in phase 3.

export class Camera {
  // Orbit angles (radians) + distance.
  private yaw = 0.6;
  private pitch = 0.3;
  private distance = 3.0;
  private readonly fov = (50 * Math.PI) / 180;
  private aspect = 1;
  private dragging = false;
  private lastX = 0;
  private lastY = 0;

  setAspect(aspect: number): void {
    this.aspect = aspect;
  }

  // Pointer handlers (left-drag = orbit, wheel = zoom; BV10 input scheme).
  onPointerDown(x: number, y: number): void {
    this.dragging = true;
    this.lastX = x;
    this.lastY = y;
  }

  onPointerUp(): void {
    this.dragging = false;
  }

  onPointerMove(x: number, y: number): void {
    if (!this.dragging) return;
    const dx = x - this.lastX;
    const dy = y - this.lastY;
    this.yaw += dx * 0.005;
    this.pitch += dy * 0.005;
    const limit = Math.PI / 2 - 0.05;
    this.pitch = Math.max(-limit, Math.min(limit, this.pitch));
    this.lastX = x;
    this.lastY = y;
  }

  onWheel(deltaY: number): void {
    this.distance *= 1 + Math.sign(deltaY) * 0.1;
    this.distance = Math.max(1.2, Math.min(20, this.distance));
  }

  // Eye position from orbit angles (orbiting the origin).
  eye(): [number, number, number] {
    const cp = Math.cos(this.pitch);
    return [
      this.distance * cp * Math.sin(this.yaw),
      this.distance * Math.sin(this.pitch),
      this.distance * cp * Math.cos(this.yaw),
    ];
  }

  // View-projection matrix (column-major, for WebGPU/WebGL). Phase-3 renderer
  // uploads this; phase 1 just keeps the math honest.
  viewProjection(): Float32Array {
    const proj = perspective(this.fov, this.aspect, 0.1, 100);
    const view = lookAt(this.eye(), [0, 0, 0], [0, 1, 0]);
    return mul(proj, view);
  }
}

// --- Minimal column-major 4x4 helpers ---
function perspective(
  fovy: number,
  aspect: number,
  near: number,
  far: number,
): Float32Array {
  const f = 1 / Math.tan(fovy / 2);
  const nf = 1 / (near - far);
  // prettier-ignore
  return new Float32Array([
    f / aspect, 0, 0, 0,
    0, f, 0, 0,
    0, 0, (far + near) * nf, -1,
    0, 0, 2 * far * near * nf, 0,
  ]);
}

function lookAt(
  eye: [number, number, number],
  center: [number, number, number],
  up: [number, number, number],
): Float32Array {
  const z = norm(sub(eye, center));
  const x = norm(cross(up, z));
  const y = cross(z, x);
  // prettier-ignore
  return new Float32Array([
    x[0], y[0], z[0], 0,
    x[1], y[1], z[1], 0,
    x[2], y[2], z[2], 0,
    -dot(x, eye), -dot(y, eye), -dot(z, eye), 1,
  ]);
}

type V3 = [number, number, number];
const sub = (a: V3, b: V3): V3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a: V3, b: V3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: V3, b: V3): V3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
function norm(a: V3): V3 {
  const l = Math.hypot(a[0], a[1], a[2]) || 1;
  return [a[0] / l, a[1] / l, a[2] / l];
}
function mul(a: Float32Array, b: Float32Array): Float32Array {
  const out = new Float32Array(16);
  for (let c = 0; c < 4; c++) {
    for (let r = 0; r < 4; r++) {
      let s = 0;
      for (let k = 0; k < 4; k++) s += a[k * 4 + r] * b[c * 4 + k];
      out[c * 4 + r] = s;
    }
  }
  return out;
}
