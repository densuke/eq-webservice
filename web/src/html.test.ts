import { test } from "node:test";
import assert from "node:assert/strict";
import { esc } from "./html.ts";

test("escapes characters that break out of text and attribute values", () => {
  assert.equal(esc(`<img src=x onerror="a('b')">&`), "&#60;img src=x onerror=&#34;a(&#39;b&#39;)&#34;&#62;&#38;");
  assert.equal(esc("茨城県南部"), "茨城県南部");
});
