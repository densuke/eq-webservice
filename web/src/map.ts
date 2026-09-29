// SVG による日本地図。外部タイルに依存せず、都道府県の塗り分け・震央・P波/S波を描く。

import type { PrefScale, TsunamiArea } from "./types.ts";
import { scaleColor, scaleLabel, scaleTextColor } from "./scale.ts";
import { interiorPoint, labelPx, mainRing, pickLabels } from "./labels.ts";
import { geoCircle } from "./waves.ts";
import { union, type Box } from "./camera.ts";
import mapCss from "./map.css";
import { esc } from "./html.ts";
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
const HOME = { lonMin: 128, lonMax: 146.2, latMin: 30, latMax: 45.8 };

/**
 * 日本全体の表示で本図に入らない離島の別枠。本図の層をそのまま縮小して映す。
 * 地震に寄っているときは本図だけで描けるので隠す。always でないものは、その範囲に何かあるときだけ出す
 */
const INSETS = [
  { id: "okinawa", title: "南西諸島", lonMin: 122.9, lonMax: 131.4, latMin: 24.0, latMax: 30.0, always: true },
  { id: "ogasawara", title: "小笠原", lonMin: 140.8, lonMax: 142.5, latMin: 24.0, latMax: 27.9, always: false },
];

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

/** project の逆 (地図座標 → 経度・緯度) */
function unproject(x: number, y: number): { lat: number; lon: number } {
  return { lon: x / KX + LON0, lat: LAT0 - y / KY };
}

export function el<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number> = {}) {
  const e = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  return e;
}

/** 点を色ごとに 1 本の path にまとめる (長さ 0 の線を丸い線端で描くと、ズームしても同じ大きさの点になる)。後の点ほど上に描く */
export function dotPaths(points: { lat: number; lon: number; color: string }[], cls: string): SVGPathElement[] {
  const byColor = new Map<string, string>();
  for (const { lat, lon, color } of points) {
    const [x, y] = project(lon, lat);
    byColor.set(color, (byColor.get(color) ?? "") + `M${x.toFixed(1)} ${y.toFixed(1)}h0`);
  }
  return [...byColor].map(([color, d]) => {
    const p = el("path", { d, class: cls });
    p.style.stroke = color;
    return p;
  });
}

async function getJson<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`地図データを取得できません (${url}: ${res.status})`);
  return res.json();
}

function polygonPath(g: GeoFeature["geometry"]): string {
  return g.coordinates.flatMap((poly) => poly.map((ring) => ringPath(ring as [number, number][]))).join("");
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
  private neighborLayer = el("g", { class: "neighbors" });
  private prefLayer = el("g", { class: "prefs" });
  private areaLayer = el("g", { class: "areas" });
  private dotLayer = el("g", { class: "dots" });
  /** 塗り分けた地域の震度の数字 */
  private labelLayer = el("g", { class: "labels" });
  /** 地域の内側に数字を置く点 (都道府県・細分区域) */
  private centers = new Map<string, [number, number]>();
  /** 都道府県の本土の外接矩形 (カメラ用。東京都の伊豆・小笠原などの離島まで映さないように) */
  private mainBoxes = new Map<string, Box>();
  private labelEls = new Map<string, SVGGElement>();
  private labelSig = "";
  /** 予測 (緊急地震速報) で塗っている都道府県・細分区域 */
  private prefForecast = new Set<string>();
  private areaForecast = new Set<string>();
  /** 続報で外された予測 (薄れながら消える) */
  private prefDropped = new Set<string>();
  private areaDropped = new Set<string>();
  private tsunamiLayer = el("g", { class: "tsunami" });
  private waveLayer = el("g", { class: "waves" });
  private markerLayer = el("g", { class: "markers" });
  private prefs = new Map<string, SVGPathElement>();
  private areas = new Map<string, SVGPathElement>();
  /** 塗り分け中の細分区域と色 */
  private areaColors = new Map<string, string>();
  /** ツールチップ用: 都道府県・細分区域の震度と、観測点の位置・震度 */
  private prefScales = new Map<string, number>();
  private areaScales = new Map<string, number>();
  private dotItems: { name: string; x: number; y: number; scale: number }[] = [];
  private tip = document.createElement("div");
  /** 自分の地点の印 */
  private home: SVGGElement | null = null;
  /** 地図で地点を選ぶ間は、次のタップ (クリック) の位置を渡す */
  private picking: ((p: { lat: number; lon: number }) => void) | null = null;
  private tsunamiAreas = new Map<string, { path: SVGPathElement; box: Box }>();
  /** 塗り分け中の都道府県と色 (薄くするときに使う) */
  private hitColors = new Map<string, string>();
  private fade = 1;
  private view: View;
  private epicenters: SVGGElement[] = [];
  /** 気象警報・注意報の区域 (市町村等のコード -> 境界)。平時だけ塗る */
  private warnLayer = el("g", { class: "warns" });
  private warns = new Map<string, SVGPathElement>();
  private warnText = new Map<string, string>();
  private warnSig = "";
  /** 平時の天気: 雨の地点 (本図の層なので別枠にも映る) と主要都市の天気 (画面上で同じ大きさ) */
  readonly rainLayer = el("g", { class: "rain" });
  readonly cityLayer = el("g", { class: "cities" });
  private feels: SVGGElement[] = [];
  private feelSig = "[]";
  private epicenterSig = "";
  private insets: ((typeof INSETS)[number] & { box: HTMLDivElement; svg: SVGSVGElement; markers: SVGGElement; bounds: Box })[] = [];
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
  /** 「全体図」のように、自動カメラを止めたうえで決まった表示へ動かしている */
  private manual = false;

  constructor(container: HTMLElement) {
    this.svg = el("svg", { class: "map", preserveAspectRatio: "xMidYMid meet" });
    // 別枠には塗り分け・津波予報・観測点の点だけを映す (震央の印は縮尺に合わせて別に描く)
    const base = el("g", { id: "map-base" });
    // 外部の CSS は <use> の複製に効かないので、図形のスタイルは SVG の中に置く
    const style = el("style");
    style.textContent = mapCss;
    base.append(style, this.neighborLayer, this.prefLayer, this.warnLayer, this.rainLayer, this.areaLayer, this.tsunamiLayer, this.dotLayer);
    this.svg.append(base, this.waveLayer, this.labelLayer, this.cityLayer, this.markerLayer);
    for (const ins of INSETS) {
      const [x0, y0] = project(ins.lonMin, ins.latMax);
      const [x1, y1] = project(ins.lonMax, ins.latMin);
      const box = document.createElement("div");
      box.className = `inset inset-${ins.id}`;
      box.hidden = true;
      box.dataset.title = ins.title;
      const svg = el("svg", { viewBox: `${x0} ${y0} ${x1 - x0} ${y1 - y0}`, preserveAspectRatio: "xMidYMid meet" });
      svg.style.aspectRatio = String((x1 - x0) / (y1 - y0));
      const use = el("use");
      use.setAttribute("href", "#map-base");
      const markers = el("g");
      svg.append(use, markers);
      box.append(svg);
      container.append(box);
      this.insets.push({ ...ins, box, svg, markers, bounds: { x0, y0, x1, y1 } });
    }
    this.waveLayer.append(this.sWave, this.pWave);
    this.tip.className = "map-tip";
    this.tip.hidden = true;
    container.append(this.tip);
    this.installTooltip();
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

  /** 周辺国の陸地 (観測範囲外。背景として描くだけ) */
  async loadNeighbors(url: string): Promise<void> {
    await this.loadPolygons(url, "neighbor", this.neighborLayer, new Map());
  }

  /** 気象警報・注意報の区域 (市町村等)。初めて平時の地図を出すときに読む */
  async loadWarningAreas(url: string): Promise<void> {
    const data = await getJson<{ features: { properties: { code: string; name: string }; geometry: GeoFeature["geometry"] }[] }>(url);
    for (const f of data.features) {
      const path = el("path", { d: polygonPath(f.geometry), class: "warn" });
      path.dataset.warn = f.properties.code;
      path.dataset.wname = f.properties.name;
      this.warnLayer.append(path);
      this.warns.set(f.properties.code, path);
    }
    this.warnSig = "";
  }

  get warningAreasLoaded(): boolean {
    return this.warns.size > 0;
  }

  /** 気象警報・注意報で塗る。空で消す */
  setWarnings(items: { code: string; level: string; text: string }[]): void {
    const sig = JSON.stringify(items);
    if (sig === this.warnSig) return;
    this.warnSig = sig;
    for (const p of this.warns.values()) delete p.dataset.level;
    this.warnText = new Map();
    for (const { code, level, text } of items) {
      const p = this.warns.get(code);
      if (!p) continue;
      p.dataset.level = level;
      this.warnText.set(code, `${p.dataset.wname}: ${text}`);
    }
  }

  /** 地震情報細分区域 (寄ったときに使う) */
  async loadAreas(url: string): Promise<void> {
    await this.loadPolygons(url, "area", this.areaLayer, this.areas);
    this.applyView();
  }

  private async loadPolygons(url: string, cls: string, layer: SVGGElement, into: Map<string, SVGPathElement>): Promise<void> {
    const data = await getJson<{ features: GeoFeature[] }>(url);
    for (const f of data.features) {
      const d = polygonPath(f.geometry);
      if (cls !== "neighbor") {
        const rings = f.geometry.coordinates.flatMap((poly) => poly.map((ring) => ring.map(([lon, lat]) => project(lon, lat))));
        this.centers.set(f.properties.name, interiorPoint(rings));
        if (cls === "pref") {
          const r = mainRing(rings);
          const xs = r.map((p) => p[0]);
          const ys = r.map((p) => p[1]);
          this.mainBoxes.set(f.properties.name, { x0: Math.min(...xs), y0: Math.min(...ys), x1: Math.max(...xs), y1: Math.max(...ys) });
        }
      }
      const path = el("path", { d, class: cls, "data-name": f.properties.name });
      const title = el("title");
      title.textContent = f.properties.name;
      path.append(title);
      layer.append(path);
      into.set(f.properties.name, path);
    }
  }

  /** 寄ったときの細かい表示: 細分区域の塗り分けと、震度観測点の点。forecast は緊急地震速報の予測 */
  /** forecast は全体を予測として塗るか。地域ごとに forecast を持たせれば、その地域だけ予測として塗る (観測と予測の重ね塗り) */
  setDetail(
    areas: { name: string; scale: number; forecast?: boolean; dropped?: boolean }[],
    forecast: boolean,
    dots: { name: string; lat: number; lon: number; scale: number }[],
  ): void {
    this.areaScales = new Map(areas.map(({ name, scale }) => [name, scale]));
    this.areaDropped = new Set(areas.filter((a) => a.dropped).map((a) => a.name));
    this.areaForecast = new Set(areas.filter((a) => !a.dropped && (a.forecast ?? forecast)).map((a) => a.name));
    this.dotItems = dots.map(({ name, lat, lon, scale }) => {
      const [x, y] = project(lon, lat);
      return { name, x, y, scale };
    });
    for (const name of this.areaColors.keys()) {
      const p = this.areas.get(name)!;
      p.style.fill = "";
      p.classList.remove("forecast", "dropped", "hit");
    }
    this.areaColors = new Map(areas.filter(({ name }) => this.areas.has(name)).map(({ name, scale }) => [name, scaleColor(scale)]));
    for (const name of this.areaColors.keys()) {
      this.areas.get(name)!.classList.toggle("forecast", this.areaForecast.has(name));
      this.areas.get(name)!.classList.toggle("dropped", this.areaDropped.has(name));
    }
    // 観測点の点 (震度の大きいものほど上に)。下に黒い縁取りを敷く
    const paths = dotPaths(
      [...dots].sort((a, b) => a.scale - b.scale).map((d) => ({ ...d, color: scaleColor(d.scale) })),
      "dot",
    );
    this.dotLayer.replaceChildren(el("path", { d: paths.map((p) => p.getAttribute("d")).join(""), class: "dot-bg" }), ...paths);
    this.paintFade();
    this.updateInsets();
  }

  /** 津波予報区の沿岸線 */
  async loadTsunami(url: string): Promise<void> {
    const data = await getJson<{ features: LineFeature[] }>(url);
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
    this.updateInsets();
  }

  /** 別枠の表示。日本全体を見ているときだけ出し、always でないものはその範囲に何かあるときだけ */
  private updateInsets(): void {
    // 細分区域で塗っている (寄っている) ときは出さない。別枠の複製は .map の下に無いので
    // .map.zoomed で都道府県の塗りを消すなどの CSS が効かず、塗りが二重になるため
    const overview = !this.svg.classList.contains("zoomed") && this.view.h >= this.homeView().h * 0.8;
    for (const ins of this.insets) {
      ins.box.hidden = !overview || !(ins.always || this.hasContentIn(ins.bounds));
      if (!ins.box.hidden) this.renderInsetMarkers(ins);
    }
  }

  /** 別枠の中の震央の印 (別枠の縮尺で、本図と同じ画面上の大きさに) */
  private renderInsetMarkers(ins: (typeof this.insets)[number]): void {
    const w = ins.svg.getBoundingClientRect().width;
    const k = w > 0 ? (ins.bounds.x1 - ins.bounds.x0) / w : 1;
    const b = ins.bounds;
    const r = 6;
    const x9 = `M${-r} ${-r}L${r} ${r}M${r} ${-r}L${-r} ${r}`;
    ins.markers.replaceChildren(
      ...this.markerItems
        .filter((m) => m.x >= b.x0 && m.x <= b.x1 && m.y >= b.y0 && m.y <= b.y1)
        .map((m) => {
          const g = el("g", { class: "epicenter sub", transform: `translate(${m.x} ${m.y}) scale(${k})` });
          g.append(el("path", { d: x9, class: "epicenter-x-bg" }), el("path", { d: x9, class: "epicenter-x" }));
          if (m.label != null) {
            const t = el("text", { x: 8, y: -6, class: "epicenter-label", style: "font-size: 11px" });
            t.textContent = String(m.label);
            g.append(t);
          }
          return g;
        }),
      // 主要都市の天気 (那覇など)
      ...[...this.cityLayer.children]
        .filter((c) => {
          const [x, y] = [Number((c as SVGElement).dataset.x), Number((c as SVGElement).dataset.y)];
          return x >= b.x0 && x <= b.x1 && y >= b.y0 && y <= b.y1;
        })
        .map((c) => {
          const g = c.cloneNode(true) as SVGGElement;
          g.setAttribute("transform", `translate(${g.dataset.x} ${g.dataset.y}) scale(${k})`);
          return g;
        }),
    );
  }

  /** 範囲内に塗り分け・津波予報・震央があるか */
  private hasContentIn(b: Box): boolean {
    const inBox = (x: number, y: number) => x >= b.x0 && x <= b.x1 && y >= b.y0 && y <= b.y1;
    const boxHits = (p: SVGGraphicsElement) => {
      const r = p.getBBox();
      return r.x <= b.x1 && r.x + r.width >= b.x0 && r.y <= b.y1 && r.y + r.height >= b.y0;
    };
    return (
      this.markerItems.some((m) => inBox(m.x, m.y)) ||
      [...this.areaColors.keys()].some((n) => boxHits(this.areas.get(n)!)) ||
      [...this.tsunamiAreas.values()].some((t) => t.path.dataset.grade && inBox((t.box.x0 + t.box.x1) / 2, (t.box.y0 + t.box.y1) / 2))
    );
  }

  /** 津波予報区の外接矩形 (地図座標) */
  tsunamiBox(names: string[]): Box | null {
    return names.reduce<Box | null>((b, n) => union(b, this.tsunamiAreas.get(n)?.box ?? null), null);
  }

  /** 都道府県の塗り分け。forecast は緊急地震速報の予測 (破線で区別) */
  setPrefScales(items: (PrefScale & { forecast?: boolean; dropped?: boolean })[], forecast = false): void {
    for (const p of this.prefs.values()) {
      p.style.fill = "";
      p.classList.remove("forecast", "dropped", "hit");
    }
    this.hitColors = new Map(items.filter(({ pref }) => this.prefs.has(pref)).map(({ pref, scale }) => [pref, scaleColor(scale)]));
    this.prefScales = new Map(items.map(({ pref, scale }) => [pref, scale]));
    this.prefDropped = new Set(items.filter((i) => i.dropped).map((i) => i.pref));
    this.prefForecast = new Set(items.filter((i) => !i.dropped && (i.forecast ?? forecast)).map((i) => i.pref));
    for (const pref of this.hitColors.keys()) {
      this.prefs.get(pref)!.classList.toggle("forecast", this.prefForecast.has(pref));
      this.prefs.get(pref)!.classList.toggle("dropped", this.prefDropped.has(pref));
    }
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
    this.labelLayer.style.opacity = String(a);
    this.renderLabels();
  }

  /**
   * 塗り分けた地域に震度の数字を出す (日本全体では都道府県、寄ったときは細分区域)。
   * 重なるものは震度の大きい方を残す。選ばれるものが変わったときだけ作り直し、ほかは位置と大きさだけ直す
   */
  private renderLabels(): void {
    const zoomed = this.svg.classList.contains("zoomed");
    const scales = zoomed ? this.areaScales : this.prefScales;
    const forecast = zoomed ? this.areaForecast : this.prefForecast;
    const dropped = zoomed ? this.areaDropped : this.prefDropped;
    const k = this.unitsPerPixel();
    const items = [...scales]
      .filter(([name, s]) => s > 0 && this.centers.has(name))
      .map(([name, scale]) => {
        const [x, y] = this.centers.get(name)!;
        const px = labelPx(scale);
        const text = scaleLabel(scale);
        const wPx = px * (text.length * 0.62 + 0.9);
        const hPx = px * 1.35;
        return { key: name, x, y, w: wPx * k, h: hPx * k, scale, text, px, wPx, hPx, forecast: forecast.has(name), dropped: dropped.has(name) };
      });
    const picked = this.fade > 0 ? pickLabels(items) : [];
    const sig = JSON.stringify(picked.map((l) => [l.key, l.scale, l.forecast, l.dropped]));
    if (sig !== this.labelSig) {
      this.labelSig = sig;
      this.labelEls = new Map(
        picked.map((l) => {
          const g = el("g", { class: l.dropped ? "label dropped" : l.forecast ? "label forecast" : "label" });
          g.dataset.x = String(l.x);
          g.dataset.y = String(l.y);
          const rect = el("rect", { x: -l.wPx / 2, y: -l.hPx / 2, width: l.wPx, height: l.hPx, rx: l.hPx * 0.28 });
          rect.style.fill = scaleColor(l.scale);
          const t = el("text", { "text-anchor": "middle", "dominant-baseline": "central", style: `font-size:${l.px}px` });
          t.style.fill = scaleTextColor(l.scale);
          t.textContent = l.text;
          g.append(rect, t);
          return [l.key, g];
        }),
      );
      this.labelLayer.replaceChildren(...this.labelEls.values());
    }
    for (const g of this.labelEls.values()) g.setAttribute("transform", `translate(${g.dataset.x} ${g.dataset.y}) scale(${k})`);
  }

  /** 震央の印。primary (表示中の地震) は大きく、ほかは小さく。label は一時的な番号、scale は最大震度 */
  setEpicenters(items: { key: string; lat: number; lon: number; label: number | null; primary: boolean; scale: number }[]): void {
    this.markerItems = items.map(({ lat, lon, ...rest }) => {
      const [x, y] = project(lon, lat);
      return { x, y, ...rest };
    });
    this.renderMarkers();
    this.updateInsets();
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
        // 押したら、まとめた中で揺れの大きい地震を選ぶ
        const key = [...labels].sort((a, b) => b.scale - a.scale)[0]?.key;
        if (key) {
          g.dataset.key = key;
          g.append(el("circle", { r: 16, class: "epicenter-hit" }));
        }
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
        return `<button type="button" class="offscreen" data-key="${esc(m.key)}" style="left:${p!.x.toFixed(0)}px;top:${p!.y.toFixed(0)}px;--c:${c}" title="${m.label}番の地震へ"><i style="transform:rotate(${p!.angle.toFixed(0)}deg) translateX(17px)"></i>${m.label}</button>`;
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
    this.manual = false;
  }

  /** 日本全体を表示する (自動カメラは止める。release() で戻る) */
  showOverview(): void {
    this.userMoved = true;
    this.manual = true;
    this.target = this.homeView();
    if (!this.moving) {
      this.moving = true;
      this.lastFrame = performance.now();
      requestAnimationFrame(this.step);
    }
  }

  /** 都道府県の本土の外接矩形 (地図座標)。離島の地震は震央の点と合わせて映す */
  prefBox(names: string[]): Box | null {
    return names.reduce<Box | null>((box, n) => union(box, this.mainBoxes.get(n) ?? null), null);
  }

  /** 目標へ指数的に近づける (目標が毎フレーム動いても滑らかに追う) */
  private step = (now: number): void => {
    const target = this.target;
    if ((this.userMoved && !this.manual) || !target) {
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
    // 画面の大きさが変わったときなどに、表示範囲の縦横比を画面に合わせる (足りない方向に広げる)
    const aspect = this.aspect();
    const cur = this.view;
    if (Math.abs(cur.w / cur.h - aspect) > 1e-3) {
      const w = Math.max(cur.w, cur.h * aspect);
      const h = w / aspect;
      this.view = { x: cur.x + (cur.w - w) / 2, y: cur.y + (cur.h - h) / 2, w, h };
    }
    const v = this.view;
    // 地図が動いたらツールチップの位置がずれるので消す
    this.tip.hidden = true;
    this.svg.setAttribute("viewBox", `${v.x} ${v.y} ${v.w} ${v.h}`);
    this.svg.classList.toggle("zoomed", this.areas.size > 0 && v.h < DETAIL_MAX_H);
    this.renderLabels();
    this.updateInsets();
    this.renderMarkers();
    this.updateMarkerScale();
  }

  /** 1 画面ピクセルあたりの地図座標 */
  private unitsPerPixel(): number {
    const r = this.svg.getBoundingClientRect();
    if (r.width === 0 || r.height === 0) return 1;
    return Math.max(this.view.w / r.width, this.view.h / r.height);
  }

  /** 自分の地点の印。null で消す */
  setHome(p: { lat: number; lon: number } | null): void {
    this.home?.remove();
    this.home = null;
    if (!p) return;
    const [x, y] = project(p.lon, p.lat);
    const g = el("g", { class: "home-marker" });
    g.dataset.x = String(x);
    g.dataset.y = String(y);
    g.dataset.k = "1";
    g.append(el("circle", { r: 9, class: "home-ring" }), el("circle", { r: 3.5, class: "home-dot" }));
    this.markerLayer.prepend(g);
    this.home = g;
    this.updateMarkerScale();
  }

  /** 次に地図をタップ (クリック) した位置を cb に渡す */
  pickPoint(cb: (p: { lat: number; lon: number }) => void): void {
    this.picking = cb;
    this.svg.classList.add("picking");
  }

  /** 地震感知情報 (利用者の「揺れた」報告) の地域の印。空で消す */
  setUserquake(items: { name: string; lat: number; lon: number; count: number; grade: string }[]): void {
    const sig = JSON.stringify(items);
    if (sig === this.feelSig) return;
    this.feelSig = sig;
    this.feels.forEach((g) => g.remove());
    this.feels = items.map(({ name, lat, lon, count, grade }) => {
      const [x, y] = project(lon, lat);
      const g = el("g", { class: `userquake grade-${grade}` });
      g.dataset.x = String(x);
      g.dataset.y = String(y);
      g.dataset.k = "1";
      const title = el("title");
      title.textContent = `${name}: 揺れの報告 ${count} 件 (信頼度 ${grade})`;
      g.append(title, el("circle", { r: 14, class: "uq-ring" }), el("circle", { r: 4, class: "uq-dot" }));
      return g;
    });
    this.markerLayer.prepend(...this.feels);
    this.updateMarkerScale();
  }

  /** 主要都市の天気を差し替えたあと、大きさと別枠を合わせる */
  refreshCities(): void {
    this.updateInsets();
    this.updateMarkerScale();
  }

  private updateMarkerScale(): void {
    const k = this.unitsPerPixel();
    const cities = [...this.cityLayer.children] as SVGGElement[];
    for (const g of [...this.epicenters, ...this.feels, ...cities, ...(this.home ? [this.home] : [])]) {
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

  /** カーソルを当てた (スマホではタップした) 場所の名前と震度 */
  private tipText(target: Element, cx: number, cy: number): string | null {
    const label = (s: number | undefined) => (s != null && s > 0 ? ` 震度${scaleLabel(s)}` : "");
    const zoomed = this.svg.classList.contains("zoomed");
    // 観測点の点は震度ごとに 1 本の path なので、近い点を探す
    if (zoomed && this.dotItems.length) {
      const [mx, my] = this.clientToMap(cx, cy);
      const r = 8 * this.unitsPerPixel();
      let best: (typeof this.dotItems)[number] | null = null;
      let bestD = r;
      for (const d of this.dotItems) {
        const dist = Math.hypot(d.x - mx, d.y - my);
        if (dist < bestD) [best, bestD] = [d, dist];
      }
      if (best) return `${best.name}${label(best.scale)}`;
    }
    const warn = target.closest<SVGElement>("[data-warn][data-level]");
    if (warn) return this.warnText.get(warn.dataset.warn ?? "") ?? null;
    const el = target.closest<SVGElement>("[data-name], .tsunami-line");
    if (!el) return null;
    if (el.classList.contains("tsunami-line")) {
      const grade = { watch: "津波注意報", warning: "津波警報", major_warning: "大津波警報" }[el.dataset.grade ?? ""];
      return grade ? `${el.textContent} ${grade}` : null;
    }
    const name = el.dataset.name ?? "";
    if (el.classList.contains("area")) return zoomed ? `${name}${label(this.areaScales.get(name))}` : null;
    if (el.classList.contains("pref")) return zoomed ? name : `${name}${label(this.prefScales.get(name))}`;
    return null;
  }

  private installTooltip(): void {
    const show = (e: PointerEvent) => {
      const text = this.tipText(e.target as Element, e.clientX, e.clientY);
      this.tip.hidden = !text;
      if (!text) return;
      const r = this.svg.getBoundingClientRect();
      this.tip.textContent = text;
      this.tip.style.left = `${e.clientX - r.left + 12}px`;
      this.tip.style.top = `${e.clientY - r.top + 12}px`;
    };
    // マウス: 動かしたとき (ボタンを押していない間)。タッチ: 動かさずに指を離したとき
    let down: { x: number; y: number } | null = null;
    this.svg.addEventListener("pointermove", (e) => {
      if (e.pointerType === "mouse" && e.buttons === 0) show(e);
      else this.tip.hidden = true;
    });
    this.svg.addEventListener("pointerleave", () => (this.tip.hidden = true));
    this.svg.addEventListener("pointerdown", (e) => (down = { x: e.clientX, y: e.clientY }));
    this.svg.addEventListener("pointerup", (e) => {
      if (e.pointerType !== "mouse" && down && Math.hypot(e.clientX - down.x, e.clientY - down.y) < 6) show(e);
      down = null;
    });
  }

  private installPanZoom(): void {
    const pointers = new Map<number, { x: number; y: number }>();
    let pinchDist = 0;

    this.svg.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        this.userMoved = true;
        this.manual = false;
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
        this.manual = false;
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
    let downAt: { x: number; y: number } | null = null;
    this.svg.addEventListener("pointerdown", (e) => (downAt = { x: e.clientX, y: e.clientY }));
    const up = (e: PointerEvent) => {
      pointers.delete(e.pointerId);
      pinchDist = 0;
      // 地点を選んでいる間は、動かさずに離した位置を渡す
      if (this.picking && e.type === "pointerup" && downAt && Math.hypot(e.clientX - downAt.x, e.clientY - downAt.y) < 6) {
        const [mx, my] = this.clientToMap(e.clientX, e.clientY);
        const cb = this.picking;
        this.picking = null;
        this.svg.classList.remove("picking");
        cb(unproject(mx, my));
      } else if (e.type === "pointerup" && downAt && Math.hypot(e.clientX - downAt.x, e.clientY - downAt.y) < 6) {
        // 震央の印を押したらその地震を選ぶ (地図が指を捕まえているので、位置から要素を探す)
        const key = document.elementFromPoint(e.clientX, e.clientY)?.closest<SVGElement>(".epicenter[data-key]")?.dataset.key;
        if (key) this.onSelect?.(key);
      }
      downAt = null;
    };
    this.svg.addEventListener("pointerup", up);
    this.svg.addEventListener("pointercancel", up);
  }
}
