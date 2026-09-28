// フロントエンドのビルド: src/main.ts を 1 ファイルにまとめ、public/ と一緒に dist/ へ出力する。
import * as esbuild from "esbuild";
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";

const watch = process.argv.includes("--watch");

rmSync("dist", { recursive: true, force: true });
mkdirSync("dist", { recursive: true });
cpSync("public", "dist", { recursive: true });

// 画面に出すバージョン (Cargo.toml の workspace.package.version)
const version = readFileSync("../Cargo.toml", "utf8").match(/^version = "(.+)"/m)[1];
writeFileSync("dist/index.html", readFileSync("dist/index.html", "utf8").replaceAll("%VERSION%", version));

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
