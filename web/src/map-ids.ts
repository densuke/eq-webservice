/** 下地の <g> の id。1 つ目は "map-base" (いまと同じ)、2 つ目からは "map-base-2"…。別枠の <use> が自分の下地を指すように分ける */
export function baseId(n: number): string {
  return n <= 1 ? "map-base" : `map-base-${n}`;
}
