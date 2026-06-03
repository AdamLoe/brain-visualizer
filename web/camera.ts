// Orbit + zoom camera (BV10, Phase 3). Produces a column-major MVP matrix,
// camera_right and camera_up vectors for the billboard shader, and handles
// left-drag orbit, wheel zoom, and touch events.
// No readback, no sim coupling (architecture §13).

export class Camera {
  private azimuth = 0.3;     // radians (spec default)
  private elevation = 0.4;   // radians (spec default)
  private distance = 3.0;    // world units from origin (spec default)
  private readonly targetPt: V3 = [0, 0, 0];
  private readonly fov = (50 * Math.PI) / 180;
  private aspect = 1;

  // Pointer state for drag-orbit.
  private lastX = 0;
  private lastY = 0;

  // Touch state for pinch-zoom.
  private lastPinchDist = 0;

  setAspect(aspect: number): void {
    this.aspect = aspect;
  }

  // --- Input handlers ---

  onPointerDown(x: number, y: number): void {
    this.lastX = x;
    this.lastY = y;
  }

  onPointerUp(): void {
    this.lastPinchDist = 0;
  }

  /** Called on mousemove. Returns true if this is an orbit move (button held),
   *  false if it is a hover event (no button) — caller routes hover to stimulate. */
  onPointerMove(x: number, y: number, buttons: number): boolean {
    if (buttons & 1) { // left drag = orbit
      const dx = x - this.lastX;
      const dy = y - this.lastY;
      this.azimuth += dx * 0.005;
      this.elevation += dy * 0.005;
      this.elevation = clamp(this.elevation, -1.4, 1.4);
      this.lastX = x;
      this.lastY = y;
      return true;
    }
    this.lastX = x;
    this.lastY = y;
    return false;
  }

  onWheel(deltaY: number): void {
    this.distance *= 1 + deltaY * 0.001;
    this.distance = clamp(this.distance, 0.5, 10.0);
  }

  // Touch: one-finger = orbit, two-finger pinch = zoom.
  onTouchStart(touches: TouchList): void {
    if (touches.length === 1) {
      this.lastX = touches[0].clientX;
      this.lastY = touches[0].clientY;
      this.lastPinchDist = 0;
    } else if (touches.length === 2) {
      const dx = touches[0].clientX - touches[1].clientX;
      const dy = touches[0].clientY - touches[1].clientY;
      this.lastPinchDist = Math.hypot(dx, dy);
    }
  }

  onTouchMove(touches: TouchList): void {
    if (touches.length === 1) {
      const t = touches[0];
      const dx = t.clientX - this.lastX;
      const dy = t.clientY - this.lastY;
      this.azimuth += dx * 0.005;
      this.elevation += dy * 0.005;
      this.elevation = clamp(this.elevation, -1.4, 1.4);
      this.lastX = t.clientX;
      this.lastY = t.clientY;
    } else if (touches.length === 2) {
      const dx = touches[0].clientX - touches[1].clientX;
      const dy = touches[0].clientY - touches[1].clientY;
      const dist = Math.hypot(dx, dy);
      if (this.lastPinchDist > 0) {
        this.distance *= this.lastPinchDist / dist;
        this.distance = clamp(this.distance, 0.5, 10.0);
      }
      this.lastPinchDist = dist;
    }
  }

  // --- Matrix + vector outputs (rebuilt each frame) ---

  /** Eye position from orbit parameters. */
  eye(): V3 {
    const cp = Math.cos(this.elevation);
    return [
      this.targetPt[0] + this.distance * cp * Math.sin(this.azimuth),
      this.targetPt[1] + this.distance * Math.sin(this.elevation),
      this.targetPt[2] + this.distance * cp * Math.cos(this.azimuth),
    ];
  }

  /** Column-major perspective * view * model MVP matrix as Float32Array. */
  mvpMatrix(): Float32Array {
    const proj = perspective(this.fov, this.aspect, 0.1, 100);
    const view = lookAt(this.eye(), this.targetPt, [0, 1, 0]);
    return mat4mul(proj, view);
  }

  /** Alias for backward compat. */
  viewProjection(): Float32Array {
    return this.mvpMatrix();
  }

  /**
   * Camera-right vector (normalised) — the billboard horizontal axis.
   * Derived from the view matrix row 0 (the camera X axis).
   */
  cameraRight(): V3 {
    // view matrix row 0 = [m0, m4, m8] (column-major layout).
    const v = lookAt(this.eye(), this.targetPt, [0, 1, 0]);
    return [v[0], v[1], v[2]];
  }

  /**
   * Camera-up vector (normalised) — the billboard vertical axis.
   * Derived from the view matrix row 1 (the camera Y axis).
   */
  cameraUp(): V3 {
    const v = lookAt(this.eye(), this.targetPt, [0, 1, 0]);
    return [v[4], v[5], v[6]];
  }

  /**
   * Unproject a screen coordinate to a world-space ray (origin + direction).
   * Used for cursor stimulation (main.ts), BV10.
   * `x`, `y` are in CSS pixel coordinates (clientX/clientY).
   * `canvasW`, `canvasH` are the canvas CSS pixel dimensions.
   */
  unproject(x: number, y: number, canvasW: number, canvasH: number): { origin: V3; dir: V3 } {
    const ndcX = (x / canvasW) * 2 - 1;
    const ndcY = 1 - (y / canvasH) * 2;

    const mvp = this.mvpMatrix();
    const inv = mat4Inverse(mvp);
    if (!inv) return { origin: [0, 0, 0], dir: [0, 0, -1] };

    const nearPt = divW(mat4MulVec4(inv, [ndcX, ndcY, -1, 1]));
    const farPt  = divW(mat4MulVec4(inv, [ndcX, ndcY,  1, 1]));
    const dir = v3norm([farPt[0] - nearPt[0], farPt[1] - nearPt[1], farPt[2] - nearPt[2]]);
    return { origin: nearPt, dir };
  }
}

// --- Minimal column-major 4×4 math helpers ---

type V3 = [number, number, number];

function perspective(fovy: number, aspect: number, near: number, far: number): Float32Array {
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

function lookAt(eye: V3, center: V3, up: V3): Float32Array {
  const z = v3norm(v3sub(eye, center));
  const x = v3norm(v3cross(up, z));
  const y = v3cross(z, x);
  // prettier-ignore
  return new Float32Array([
    x[0], y[0], z[0], 0,
    x[1], y[1], z[1], 0,
    x[2], y[2], z[2], 0,
    -v3dot(x, eye), -v3dot(y, eye), -v3dot(z, eye), 1,
  ]);
}

const v3sub = (a: V3, b: V3): V3 => [a[0]-b[0], a[1]-b[1], a[2]-b[2]];
const v3dot = (a: V3, b: V3): number => a[0]*b[0] + a[1]*b[1] + a[2]*b[2];
const v3cross = (a: V3, b: V3): V3 => [
  a[1]*b[2] - a[2]*b[1],
  a[2]*b[0] - a[0]*b[2],
  a[0]*b[1] - a[1]*b[0],
];
function v3norm(a: V3): V3 {
  const l = Math.hypot(a[0], a[1], a[2]) || 1;
  return [a[0]/l, a[1]/l, a[2]/l];
}
function mat4mul(a: Float32Array, b: Float32Array): Float32Array {
  const out = new Float32Array(16);
  for (let c = 0; c < 4; c++) {
    for (let r = 0; r < 4; r++) {
      let s = 0;
      for (let k = 0; k < 4; k++) s += a[k*4+r] * b[c*4+k];
      out[c*4+r] = s;
    }
  }
  return out;
}
function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

/** Invert a column-major 4×4 matrix. Returns null if singular. */
function mat4Inverse(m: Float32Array): Float32Array | null {
  const inv = new Float32Array(16);
  inv[0]  =  m[5]*m[10]*m[15] - m[5]*m[11]*m[14] - m[9]*m[6]*m[15]  + m[9]*m[7]*m[14]  + m[13]*m[6]*m[11]  - m[13]*m[7]*m[10];
  inv[4]  = -m[4]*m[10]*m[15] + m[4]*m[11]*m[14] + m[8]*m[6]*m[15]  - m[8]*m[7]*m[14]  - m[12]*m[6]*m[11]  + m[12]*m[7]*m[10];
  inv[8]  =  m[4]*m[9] *m[15] - m[4]*m[11]*m[13] - m[8]*m[5]*m[15]  + m[8]*m[7]*m[13]  + m[12]*m[5]*m[11]  - m[12]*m[7]*m[9];
  inv[12] = -m[4]*m[9] *m[14] + m[4]*m[10]*m[13] + m[8]*m[5]*m[14]  - m[8]*m[6]*m[13]  - m[12]*m[5]*m[10]  + m[12]*m[6]*m[9];
  inv[1]  = -m[1]*m[10]*m[15] + m[1]*m[11]*m[14] + m[9]*m[2]*m[15]  - m[9]*m[3]*m[14]  - m[13]*m[2]*m[11]  + m[13]*m[3]*m[10];
  inv[5]  =  m[0]*m[10]*m[15] - m[0]*m[11]*m[14] - m[8]*m[2]*m[15]  + m[8]*m[3]*m[14]  + m[12]*m[2]*m[11]  - m[12]*m[3]*m[10];
  inv[9]  = -m[0]*m[9] *m[15] + m[0]*m[11]*m[13] + m[8]*m[1]*m[15]  - m[8]*m[3]*m[13]  - m[12]*m[1]*m[11]  + m[12]*m[3]*m[9];
  inv[13] =  m[0]*m[9] *m[14] - m[0]*m[10]*m[13] - m[8]*m[1]*m[14]  + m[8]*m[2]*m[13]  + m[12]*m[1]*m[10]  - m[12]*m[2]*m[9];
  inv[2]  =  m[1]*m[6] *m[15] - m[1]*m[7] *m[14] - m[5]*m[2]*m[15]  + m[5]*m[3]*m[14]  + m[13]*m[2]*m[7]   - m[13]*m[3]*m[6];
  inv[6]  = -m[0]*m[6] *m[15] + m[0]*m[7] *m[14] + m[4]*m[2]*m[15]  - m[4]*m[3]*m[14]  - m[12]*m[2]*m[7]   + m[12]*m[3]*m[6];
  inv[10] =  m[0]*m[5] *m[15] - m[0]*m[7] *m[13] - m[4]*m[1]*m[15]  + m[4]*m[3]*m[13]  + m[12]*m[1]*m[7]   - m[12]*m[3]*m[5];
  inv[14] = -m[0]*m[5] *m[14] + m[0]*m[6] *m[13] + m[4]*m[1]*m[14]  - m[4]*m[2]*m[13]  - m[12]*m[1]*m[6]   + m[12]*m[2]*m[5];
  inv[3]  = -m[1]*m[6] *m[11] + m[1]*m[7] *m[10] + m[5]*m[2]*m[11]  - m[5]*m[3]*m[10]  - m[9] *m[2]*m[7]   + m[9] *m[3]*m[6];
  inv[7]  =  m[0]*m[6] *m[11] - m[0]*m[7] *m[10] - m[4]*m[2]*m[11]  + m[4]*m[3]*m[10]  + m[8] *m[2]*m[7]   - m[8] *m[3]*m[6];
  inv[11] = -m[0]*m[5] *m[11] + m[0]*m[7] *m[9]  + m[4]*m[1]*m[11]  - m[4]*m[3]*m[9]   - m[8] *m[1]*m[7]   + m[8] *m[3]*m[5];
  inv[15] =  m[0]*m[5] *m[10] - m[0]*m[6] *m[9]  - m[4]*m[1]*m[10]  + m[4]*m[2]*m[9]   + m[8] *m[1]*m[6]   - m[8] *m[2]*m[5];
  const det = m[0]*inv[0] + m[1]*inv[4] + m[2]*inv[8] + m[3]*inv[12];
  if (Math.abs(det) < 1e-15) return null;
  const d = 1 / det;
  for (let i = 0; i < 16; i++) inv[i] *= d;
  return inv;
}

function mat4MulVec4(m: Float32Array, v: [number,number,number,number]): [number,number,number,number] {
  return [
    m[0]*v[0] + m[4]*v[1] + m[8]*v[2]  + m[12]*v[3],
    m[1]*v[0] + m[5]*v[1] + m[9]*v[2]  + m[13]*v[3],
    m[2]*v[0] + m[6]*v[1] + m[10]*v[2] + m[14]*v[3],
    m[3]*v[0] + m[7]*v[1] + m[11]*v[2] + m[15]*v[3],
  ];
}

function divW(v: [number,number,number,number]): V3 {
  const w = v[3] || 1;
  return [v[0]/w, v[1]/w, v[2]/w];
}
