"""simplify_geojson の南西諸島の小さな輪の扱いの確かめ (python3 tools/test_simplify_geojson.py)"""

from simplify_geojson import simplify_ring


def square(x, y, d):
    return [(x, y), (x + d, y), (x + d, y + d), (x, y + d), (x, y)]


def test_small_ring():
    tiny = 0.012  # 約 1.3km 四方 (約 1.4km²)。MIN_AREA (0.0004) 未満・NANSEI_MIN_AREA (0.00005) 以上
    assert simplify_ring(square(129.5, 29.5, tiny)) is not None  # 南西諸島では残る
    assert simplify_ring(square(139.5, 35.5, tiny)) is None  # 全国では今までどおり捨てる
    assert simplify_ring(square(129.5, 29.5, 0.005)) is None  # 0.6km² 未満は南西諸島でも捨てる


if __name__ == "__main__":
    test_small_ring()
    print("ok")
