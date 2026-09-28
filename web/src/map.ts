// SVG による日本地図。外部タイルに依存せず、都道府県の塗り分け・震央・P波/S波を描く。

import type { PrefScale } from "./types.ts";
import { scaleColor } from "./scale.ts";
import { geoCircle } from "./waves.ts";

const SVG_NS = "http://www.w3.org/2000/svg";
const LON0 = 137;
const LAT0 = 37;
const KX = Math.cos((LAT0 * Math.PI) / 180) * 100;
const KY = 100;

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
  private waveLayer = el("g", { class: "waves" });
  private markerLayer = el("g", { class: "markers" });
  private prefs = new Map<string, SVGPathElement>();
  private view: View;
  private epicenter: SVGGElement | null = null;
  private pWave = el("path", { class: "wave wave-p" });
  private sWave = el("path", { class: "wave wave-s" });
  /** 利用者が手で動かしたら自動フォーカスを控える */
  userMoved = false;

  constructor(container: HTMLElement) {
    this.svg = el("svg", { class: "map", preserveAspectRatio: "xMidYMid meet" });
    this.svg.append(this.prefLayer, this.waveLayer, this.markerLayer);
    this.waveLayer.append(this.sWave, this.pWave);
    container.append(this.svg);
    this.view = this.homeView();
    this.applyView();
    this.installPanZoom();
    new ResizeObserver(() => this.applyView()).observe(this.svg);
  }

  async load(url: string): Promise<void> {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`地図データを取得できません (${res.status})`);
    const data: { features: GeoFeature[] } = await res.json();
    for (const f of data.features) {
      const d = f.geometry.coordinates.flatMap((poly) => poly.map((ring) => ringPath(ring as [number, number][]))).join("");
      const path = el("path", { d, class: "pref", "data-name": f.properties.name });
      const title = el("title");
      title.textContent = f.properties.name;
      path.append(title);
      this.prefLayer.append(path);
      this.prefs.set(f.properties.name, path);
    }
  }

  /** 都道府県の塗り分け。forecast は緊急地震速報の予測 (破線で区別) */
  setPrefScales(items: PrefScale[], forecast = false): void {
    for (const p of this.prefs.values()) {
      p.style.fill = "";
      p.classList.remove("forecast", "hit");
    }
    for (const { pref, scale } of items) {
      const p = this.prefs.get(pref);
      if (!p) continue;
      p.style.fill = scaleColor(scale);
      p.classList.add("hit");
      if (forecast) p.classList.add("forecast");
    }
  }

  setEpicenter(lat: number | null, lon: number | null): void {
    this.epicenter?.remove();
    this.epicenter = null;
    if (lat == null || lon == null) return;
    const [x, y] = project(lon, lat);
    const g = el("g", { class: "epicenter" });
    g.dataset.x = String(x);
    g.dataset.y = String(y);
    const r = 9;
    g.append(
      el("circle", { r: 16, class: "epicenter-pulse" }),
      el("path", { d: `M${-r} ${-r}L${r} ${r}M${r} ${-r}L${-r} ${r}`, class: "epicenter-x-bg" }),
      el("path", { d: `M${-r} ${-r}L${r} ${r}M${r} ${-r}L${-r} ${r}`, class: "epicenter-x" }),
    );
    this.markerLayer.append(g);
    this.epicenter = g;
    this.updateMarkerScale();
  }

  /** P波・S波の到達範囲 (km)。null は非表示 */
  setWaves(center: { lat: number; lon: number } | null, pKm: number | null, sKm: number | null): void {
    const circle = (km: number | null) => {
      if (!center || km == null || km <= 0) return "";
      return ringPath(geoCircle(center.lat, center.lon, km));
    };
    this.pWave.setAttribute("d", circle(pKm));
    this.sWave.setAttribute("d", circle(sKm));
  }

  /** 震央付近へ移動する (spanKm 四方程度が見えるように) */
  focus(lat: number, lon: number, spanKm = 700): void {
    if (this.userMoved) return;
    const [x, y] = project(lon, lat);
    const h = (spanKm / 111) * KY;
    const aspect = this.aspect();
    const w = h * aspect;
    this.animateTo({ x: x - w / 2, y: y - h / 2, w, h });
  }

  resetView(): void {
    this.userMoved = false;
    this.animateTo(this.homeView());
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

  private animateTo(target: View): void {
    const from = { ...this.view };
    const start = performance.now();
    const dur = 600;
    const step = (now: number) => {
      const t = Math.min(1, (now - start) / dur);
      const e = 1 - (1 - t) ** 3;
      this.view = {
        x: from.x + (target.x - from.x) * e,
        y: from.y + (target.y - from.y) * e,
        w: from.w + (target.w - from.w) * e,
        h: from.h + (target.h - from.h) * e,
      };
      this.applyView();
      if (t < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }

  private applyView(): void {
    const v = this.view;
    this.svg.setAttribute("viewBox", `${v.x} ${v.y} ${v.w} ${v.h}`);
    this.updateMarkerScale();
  }

  /** 1 画面ピクセルあたりの地図座標 */
  private unitsPerPixel(): number {
    const r = this.svg.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) return 1;
    return Math.max(this.view.w / r.width, this.view.h / r.height);
  }

  private updateMarkerScale(): void {
    if (!this.epicenter) return;
    const k = this.unitsPerPixel();
    const { x, y } = this.epicenter.dataset;
    this.epicenter.setAttribute("transform", `translate(${x} ${y}) scale(${k})`);
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
