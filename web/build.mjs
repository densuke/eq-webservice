// フロントエンドのビルド: src/main.ts を 1 ファイルにまとめ、public/ と一緒に dist/ へ出力する。
import * as esbuild from "esbuild";
import { cpSync, mkdirSync, rmSync } from "node:fs";

const watch = process.argv.includes("--watch");

rmSync("dist", { recursive: true, force: true });
mkdirSync("dist", { recursive: true });
cpSync("public", "dist", { recursive: true });

const options = {
  entryPoints: ["src/main.ts"],
  bundle: true,
  minify: !watch,
  sourcemap: true,
  target: ["es2022"],
  format: "esm",
  outfile: "dist/app.js",
  logLevel: "info",
};

if (watch) {
  const ctx = await esbuild.context(options);
  await ctx.watch();
} else {
  await esbuild.build(options);
}
