import { test } from "node:test";
import assert from "node:assert/strict";
import { wsUrl } from "./ws-url.ts";

test("the ws url follows the page (scheme and sub path)", () => {
  assert.equal(wsUrl("https://eq.fuga.jp/"), "wss://eq.fuga.jp/ws");
  assert.equal(wsUrl("http://localhost:8080/eq/?layout=x"), "ws://localhost:8080/eq/ws");
});

test("the broadcast page names itself so it is not counted as a web viewer", () => {
  assert.equal(wsUrl("https://eq.fuga.jp/?broadcast=1&audio=mixer"), "wss://eq.fuga.jp/ws?broadcast=1");
});
