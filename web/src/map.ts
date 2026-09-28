// SVG による日本地図。外部タイルに依存せず、都道府県の塗り分け・震央・P波/S波を描く。

import type { PrefScale, TsunamiArea } from "./types.ts";
import { scaleColor } from "./scale.ts";
import { geoCircle } from "./waves.ts";
import { union, type Box } from "./camera.ts";
import { clusterMarkers, edgePoint, labelSize, type Cluster, type Marker } from "./cluster.ts";

const SVG_NS = "http://www.w3.org/2000/svg";
const LON0 = 137;
const LAT0 = 37;
const KX = Math.cos((LAT0 * Math.PI) / 180) * 100;
const KY = 100;

/** 震央の印をまとめる距離 (画面上の px) */
const MARKER_MERGE_PX = 18;

/** 表示範囲の高さがこれより小さい (寄っている) ときは、細分区域と観測点で描く (1 度 ≒ 100) */
const DETAIL_MAX_H = 1300;

/** 日本全体が収まる表示範囲 (lon/lat) */
const HOME = { lonMin: 122.5, lonMax: 149, latMin: 24, latMax: 46 };

interface View {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface GeoFeature {
  properties: { name: string };
  geometry: { type: "MultiPolygon"; coordinates: number[][][][] };
}

interface LineFeature {
  properties: { name: string };
  geometry: { type: "MultiLineString"; coordinates: number[][][] };
}

export function project(lon: number, lat: number): [number, number] {
  return [(lon - LON0) * KX, -(lat - LAT0) * KY];
}

function el<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number> = {}) {
  const e = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  return e;
}

function ringPath(coords: [number, number][]): string {
  let d = "";
  coords.forEach(([lon, lat], i) => {
    const [x, y] = project(lon, lat);
    d += `${i === 0 ? "M" : "L"}${x.toFixed(1)} ${y.toFixed(1)}`;
  });
  return d + "Z";
}

export class JapanMap {
  readonly svg: SVGSVGElement;
  private prefLayer = el("g", { class: "prefs" });
  private areaLayer = el("g", { class: "areas" });
  private dotLayer = el("g", { class: "dots" });
  private tsunamiLayer = el("g", { class: "tsunami" });
  private waveLayer = el("g", { class: "waves" });
  private markerLayer = el("g", { class: "markers" });
  private prefs = new Map<string, SVGPathElement>();
  private areas = new Map<string, SVGPathElement>();
  /** 塗り分け中の細分区域と色 */
  private areaColors = new Map<string, string>();
  private tsunamiAreas = new Map<string, { path: SVGPathElement; box: Box }>();
  /** 塗り分け中の都道府県と色 (薄くするときに使う) */
  private hitColors = new Map<string, string>();
  private fade = 1;
  private view: View;
  private epicenters: SVGGElement[] = [];
  private epicenterSig = "";
  private markerItems: { key: string; x: number; y: number; label: number | null; primary: boolean; scale: number }[] = [];
  /** 画面外の地震の方向を示す矢印 (地図の上に重ねる HTML) */
  private offscreen = document.createElement("div");
  /** 画面外の矢印が押されたとき (グループのキー) */
  onSelect: ((key: string) => void) | null = null;
  private pWave = el("path", { class: "wave wave-p" });
  private sWave = el("path", { class: "wave wave-s" });
  private target: View | null = null;
  private moving = false;
  private lastFrame = 0;
  /** 利用者が手で動かしたら自動カメラを止める */
  userMoved = false;

  constructor(container: HTMLElement) {
    this.svg = el("svg", { class: "map", preserveAspectRatio: "xMidYMid meet" });
    this.svg.append(this.prefLayer, this.areaLayer, this.tsunamiLayer, this.waveLayer, this.dotLayer, this.markerLayer);
    this.waveLayer.append(this.sWave, this.pWave);
    this.offscreen.className = "offscreen-layer";
    this.offscreen.addEventListener("click", (e) => {
      const key = (e.target as HTMLElement).closest<HTMLElement>("[data-key]")?.dataset.key;
      if (key) this.onSelect?.(key);
    });
    container.append(this.svg, this.offscreen);
    this.view = this.homeView();
    this.applyView();
    this.installPanZoom();
    new ResizeObserver(() => this.applyView()).observe(this.svg);
  }

  async load(url: string): Promise<void> {
    await this.loadPolygons(url, "pref", this.prefLayer, this.prefs);
  }

  /** 地震情報細分区域 (寄ったときに使う) */
  async loadAreas(url: string): Promise<void> {
    await this.loadPolygons(url, "area", this.areaLayer, this.areas);
    this.applyView();
  }

  private async loadPolygons(url: string, cls: string, layer: SVGGElement, into: Map<string, SVGPathElement>): Promise<void> {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`地図データを取得できません (${url}: ${res.status})`);
    const data: { features: GeoFeature[] } = await res.json();
    for (const f of data.features) {
      const d = f.geometry.coordinates.flatMap((poly) => poly.map((ring) => ringPath(ring as [number, number][]))).join("");
      const path = el("path", { d, class: cls, "data-name": f.properties.name });
      const title = el("title");
      title.textContent = f.properties.name;
      path.append(title);
      layer.append(path);
      into.set(f.properties.name, path);
    }
  }

  /** 寄ったときの細かい表示: 細分区域の塗り分けと、震度観測点の点。forecast は緊急地震速報の予測 */
  setDetail(areas: { name: string; scale: number }[], forecast: boolean, dots: { lat: number; lon: number; scale: number }[]): void {
    for (const name of this.areaColors.keys()) {
      const p = this.areas.get(name)!;
      p.style.fill = "";
      p.classList.remove("forecast", "hit");
    }
    this.areaColors = new Map(areas.filter(({ name }) => this.areas.has(name)).map(({ name, scale }) => [name, scaleColor(scale)]));
    for (const name of this.areaColors.keys()) this.areas.get(name)!.classList.toggle("forecast", forecast);
    // 点は震度ごとに 1 本の path にまとめる (長さ 0 の線を丸い線端で描くと、ズームしても同じ大きさの点になる)
    const byScale = new Map<number, string>();
    for (const { lat, lon, scale } of [...dots].sort((a, b) => a.scale - b.scale)) {
      const [x, y] = project(lon, lat);
      byScale.set(scale, (byScale.get(scale) ?? "") + `M${x.toFixed(1)} ${y.toFixed(1)}h0`);
    }
    const all = [...byScale.values()].join("");
    this.dotLayer.replaceChildren(
      el("path", { d: all, class: "dot-bg" }),
      ...[...byScale].map(([scale, d]) => {
        const p = el("path", { d, class: "dot" });
        p.style.stroke = scaleColor(scale);
        return p;
      }),
    );
    this.paintFade();
  }

  /** 津波予報区の沿岸線 */
  async loadTsunami(url: string): Promise<void> {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`津波予報区のデータを取得できません (${res.status})`);
    const data: { features: LineFeature[] } = await res.json();
    for (const f of data.features) {
      let d = "";
      let box: Box | null = null;
      for (const line of f.geometry.coordinates) {
        line.forEach(([lon, lat], i) => {
          const [x, y] = project(lon, lat);
          d += `${i === 0 ? "M" : "L"}${x.toFixed(1)} ${y.toFixed(1)}`;
          box = union(box, { x0: x, y0: y, x1: x, y1: y });
        });
      }
      const path = el("path", { d, class: "tsunami-line" });
      const title = el("title");
      title.textContent = f.properties.name;
      path.append(title);
      this.tsunamiLayer.append(path);
      this.tsunamiAreas.set(f.properties.name, { path, box: box! });
    }
  }

  /** 発表中の津波予報区を等級の色で描く */
  setTsunami(areas: TsunamiArea[]): void {
    for (const { path } of this.tsunamiAreas.values()) delete path.dataset.grade;
    for (const a of areas) {
      const t = this.tsunamiAreas.get(a.name);
      if (t) t.path.dataset.grade = a.grade;
    }
  }

  /** 津波予報区の外接矩形 (地図座標) */
  tsunamiBox(names: string[]): Box | null {
    return names.reduce<Box | null>((b, n) => union(b, this.tsunamiAreas.get(n)?.box ?? null), null);
  }

  /** 都道府県の塗り分け。forecast は緊急地震速報の予測 (破線で区別) */
  setPrefScales(items: PrefScale[], forecast = false): void {
    for (const p of this.prefs.values()) {
      p.style.fill = "";
      p.classList.remove("forecast", "hit");
    }
    this.hitColors = new Map(items.filter(({ pref }) => this.prefs.has(pref)).map(({ pref, scale }) => [pref, scaleColor(scale)]));
    for (const pref of this.hitColors.keys()) this.prefs.get(pref)!.classList.toggle("forecast", forecast);
    this.paintFade();
  }

  /** 塗り分けと震央の濃さ (1 = はっきり, 0 = 消える) */
  setFade(alpha: number): void {
    if (Math.abs(alpha - this.fade) < 0.005) return;
    this.fade = alpha;
    this.paintFade();
  }

  private paintFade(): void {
    const a = this.fade;
    const mix = (color: string) => (a >= 1 ? color : `color-mix(in srgb, ${color} ${(a * 100).toFixed(1)}%, var(--land))`);
    for (const [pref, color] of this.hitColors) {
      const p = this.prefs.get(pref)!;
      p.style.fill = mix(color);
      p.classList.toggle("hit", a > 0);
    }
    for (const [name, color] of this.areaColors) {
      const p = this.areas.get(name)!;
      p.style.fill = mix(color);
      p.classList.toggle("hit", a > 0);
    }
    this.markerLayer.style.opacity = String(a);
    this.dotLayer.style.opacity = String(a);
  }

  /** 震央の印。primary (表示中の地震) は大きく、ほかは小さく。label は一時的な番号、scale は最大震度 */
  setEpicenters(items: { key: string; lat: number; lon: number; label: number | null; primary: boolean; scale: number }[]): void {
    this.markerItems = items.map(({ lat, lon, ...rest }) => {
      const [x, y] = project(lon, lat);
      return { x, y, ...rest };
    });
    this.renderMarkers();
  }

  /** 画面上で重なる印をまとめて描く。まとまり方はズームで変わるので、表示範囲が変わるたびに呼ぶ */
  private renderMarkers(): void {
    this.renderOffscreen();
    const labeled = this.markerItems.flatMap(({ label, ...m }): Marker[] => (label == null ? [] : [{ ...m, label }]));
    const clusters: Cluster[] = [
      ...clusterMarkers(labeled, MARKER_MERGE_PX * this.unitsPerPixel()),
      ...this.markerItems.filter((m) => m.label == null).map(({ x, y, primary }) => ({ x, y, primary, labels: [] })),
    ];
    const sig = JSON.stringify(clusters.map((c) => [c.x, c.y, c.primary, c.labels]));
    if (sig === this.epicenterSig) return;
    this.epicenterSig = sig;
    this.epicenters.forEach((g) => g.remove());
    // 表示中の地震を最前面に
    this.epicenters = clusters
      .sort((a, b) => Number(a.primary) - Number(b.primary))
      .map(({ x, y, primary, labels }) => {
        const g = el("g", { class: primary ? "epicenter" : "epicenter sub" });
        g.dataset.x = String(x);
        g.dataset.y = String(y);
        g.dataset.k = primary ? "1" : "0.65";
        const r = 9;
        const x9 = `M${-r} ${-r}L${r} ${r}M${r} ${-r}L${-r} ${r}`;
        if (primary) g.append(el("circle", { r: 16, class: "epicenter-pulse" }));
        g.append(el("path", { d: x9, class: "epicenter-x-bg" }), el("path", { d: x9, class: "epicenter-x" }));
        if (labels.length > 0) {
          // 番号を時系列順に並べ、揺れの大きい地震ほど大きな文字にする
          const t = el("text", { x: 11, y: -9, class: "epicenter-label" });
          labels.forEach(({ label, scale }, i) => {
            if (i > 0) t.append(",");
            const s = el("tspan", { "font-size": labelSize(scale) });
            s.textContent = String(label);
            t.append(s);
          });
          g.append(t);
        }
        this.markerLayer.append(g);
        return g;
      });
    this.updateMarkerScale();
  }

  /** 番号の付いた地震の震央が画面外にあれば、画面の端にその方向の矢印と番号を出す */
  private renderOffscreen(): void {
    const r = this.svg.getBoundingClientRect();
    const k = this.unitsPerPixel();
    const ox = this.view.x - (r.width * k - this.view.w) / 2;
    const oy = this.view.y - (r.height * k - this.view.h) / 2;
    const html = this.markerItems
      .filter((m) => m.label != null)
      .map((m) => ({ m, p: edgePoint((m.x - ox) / k, (m.y - oy) / k, r.width, r.height, 22) }))
      .filter(({ p }) => p)
      .map(({ m, p }) => {
        const c = scaleColor(m.scale);
        return `<button type="button" class="offscreen" data-key="${m.key.replace(/"/g, "&quot;")}" style="left:${p!.x.toFixed(0)}px;top:${p!.y.toFixed(0)}px;--c:${c}" title="${m.label}番の地震へ"><i style="transform:rotate(${p!.angle.toFixed(0)}deg) translateX(17px)"></i>${m.label}</button>`;
      })
      .join("");
    if (this.offscreen.innerHTML !== html) this.offscreen.innerHTML = html;
  }

  /** P波・S波の到達範囲 (km)。複数の地震の円をまとめて描く。空配列で非表示 */
  setWaves(waves: { lat: number; lon: number; pKm: number | null; sKm: number | null }[]): void {
    const circles = (km: (w: (typeof waves)[number]) => number | null) =>
      waves
        .map((w) => {
          const r = km(w);
          return r != null && r > 0 ? ringPath(geoCircle(w.lat, w.lon, r)) : "";
        })
        .join("");
    this.pWave.setAttribute("d", circles((w) => w.pKm));
    this.sWave.setAttribute("d", circles((w) => w.sKm));
  }

  /** 自動カメラの目標。null は日本全体。利用者が手で動かしている間は何もしない */
  setTarget(box: Box | null): void {
    if (this.userMoved) return;
    this.target = box ? this.boxToView(box) : this.homeView();
    if (!this.moving) {
      this.moving = true;
      this.lastFrame = performance.now();
      requestAnimationFrame(this.step);
    }
  }

  /** 自動カメラに戻す */
  release(): void {
    this.userMoved = false;
  }

  /** 都道府県の外接矩形 (地図座標) */
  prefBox(names: string[]): Box | null {
    let box: Box | null = null;
    for (const n of names) {
      const b = this.prefs.get(n)?.getBBox();
      if (b) box = union(box, { x0: b.x, y0: b.y, x1: b.x + b.width, y1: b.y + b.height });
    }
    return box;
  }

  /** 目標へ指数的に近づける (目標が毎フレーム動いても滑らかに追う) */
  private step = (now: number): void => {
    const target = this.target;
    if (this.userMoved || !target) {
      this.moving = false;
      return;
    }
    const k = 1 - Math.exp(-(now - this.lastFrame) / 250);
    this.lastFrame = now;
    const v = this.view;
    const eps = v.w * 0.002;
    const near = Math.max(Math.abs(target.x - v.x), Math.abs(target.y - v.y), Math.abs(target.w - v.w), Math.abs(target.h - v.h)) < eps;
    this.view = near
      ? { ...target }
      : { x: v.x + (target.x - v.x) * k, y: v.y + (target.y - v.y) * k, w: v.w + (target.w - v.w) * k, h: v.h + (target.h - v.h) * k };
    this.applyView();
    if (near) this.moving = false;
    else requestAnimationFrame(this.step);
  };

  /** 画面の縦横比に合わせて box 全体が収まる表示範囲 */
  private boxToView(b: Box): View {
    const aspect = this.aspect();
    const w = Math.max(b.x1 - b.x0, (b.y1 - b.y0) * aspect);
    const h = w / aspect;
    const cx = (b.x0 + b.x1) / 2;
    const cy = (b.y0 + b.y1) / 2;
    return { x: cx - w / 2, y: cy - h / 2, w, h };
  }

  private homeView(): View {
    const [x0, y0] = project(HOME.lonMin, HOME.latMax);
    const [x1, y1] = project(HOME.lonMax, HOME.latMin);
    return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
  }

  private aspect(): number {
    const r = this.svg.getBoundingClientRect();
    return r.width > 0 && r.height > 0 ? r.width / r.height : 1;
  }

  private applyView(): void {
    const v = this.view;
    this.svg.setAttribute("viewBox", `${v.x} ${v.y} ${v.w} ${v.h}`);
    this.svg.classList.toggle("detail", this.areas.size > 0 && v.h < DETAIL_MAX_H);
    this.renderMarkers();
    this.updateMarkerScale();
  }

  /** 1 画面ピクセルあたりの地図座標 */
  private unitsPerPixel(): number {
    const r = this.svg.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) return 1;
    return Math.max(this.view.w / r.width, this.view.h / r.height);
  }

  private updateMarkerScale(): void {
    const k = this.unitsPerPixel();
    for (const g of this.epicenters) {
      const { x, y, k: size } = g.dataset;
      g.setAttribute("transform", `translate(${x} ${y}) scale(${k * Number(size)})`);
    }
  }

  private clientToMap(cx: number, cy: number): [number, number] {
    const r = this.svg.getBoundingClientRect();
    const k = this.unitsPerPixel();
    // preserveAspectRatio="xMidYMid meet" の余白を考慮
    const ox = this.view.x - (r.width * k - this.view.w) / 2;
    const oy = this.view.y - (r.height * k - this.view.h) / 2;
    return [ox + (cx - r.left) * k, oy + (cy - r.top) * k];
  }

  private zoomAt(cx: number, cy: number, factor: number): void {
    const [mx, my] = this.clientToMap(cx, cy);
    const w = Math.min(Math.max(this.view.w * factor, 40), 5000);
    const f = w / this.view.w;
    this.view = {
      x: mx - (mx - this.view.x) * f,
      y: my - (my - this.view.y) * f,
      w,
      h: this.view.h * f,
    };
    this.applyView();
  }

  private installPanZoom(): void {
    const pointers = new Map<number, { x: number; y: number }>();
    let pinchDist = 0;

    this.svg.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        this.userMoved = true;
        this.zoomAt(e.clientX, e.clientY, Math.exp(e.deltaY * 0.0015));
      },
      { passive: false },
    );
    this.svg.addEventListener("pointerdown", (e) => {
      this.svg.setPointerCapture(e.pointerId);
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pointers.size === 2) {
        const [a, b] = [...pointers.values()];
        pinchDist = Math.hypot(a.x - b.x, a.y - b.y);
      }
    });
    this.svg.addEventListener("pointermove", (e) => {
      const prev = pointers.get(e.pointerId);
      if (!prev) return;
      this.userMoved = true;
      if (pointers.size === 1) {
        const k = this.unitsPerPixel();
        this.view.x -= (e.clientX - prev.x) * k;
        this.view.y -= (e.clientY - prev.y) * k;
        pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
        this.applyView();
      } else if (pointers.size === 2) {
        pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
        const [a, b] = [...pointers.values()];
        const d = Math.hypot(a.x - b.x, a.y - b.y);
        if (pinchDist > 0 && d > 0) this.zoomAt((a.x + b.x) / 2, (a.y + b.y) / 2, pinchDist / d);
        pinchDist = d;
      }
    });
    const up = (e: PointerEvent) => {
      pointers.delete(e.pointerId);
      pinchDist = 0;
    };
    this.svg.addEventListener("pointerup", up);
    this.svg.addEventListener("pointercancel", up);
  }
}
