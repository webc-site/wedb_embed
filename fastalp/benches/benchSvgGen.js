#!/usr/bin/env -S bun
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { parse as yamlParse } from "yaml";
import { loadBenchData } from "./lib/data.js";
import { renderSvg } from "./lib/render.js";
import { optimizeSvg, renderJpg } from "./lib/upload.js";

const BENCHES_DIR = import.meta.dirname;
const I18N_DIR = resolve(BENCHES_DIR, "i18n");
const IMG_DIR = resolve(BENCHES_DIR, "img");
const LANG_LI = ["zh", "en"];

const i18nLoad = async (lang = "zh") => {
  const ymlPath = resolve(I18N_DIR, `${lang}.yml`);
  const content = await Bun.file(ymlPath).text();
  return yamlParse(content);
};

export const benchSvgGen = async () => {
  console.log("1. Loading benchmark dataset JSONs...");
  const benchData = await loadBenchData();

  for (const lang of LANG_LI) {
    console.log(`\n2. Generating SVG and JPG for [${lang}] (Mobile Layout)...`);
    const i18n = await i18nLoad(lang);
    const rawSvg = renderSvg(benchData, i18n, lang);
    const svg = optimizeSvg(rawSvg);

    const outDir = resolve(IMG_DIR, lang);
    await mkdir(outDir, { recursive: true });

    const localSvg = resolve(outDir, "bench.svg");
    const localJpg = resolve(outDir, "bench.jpg");

    await Bun.write(localSvg, svg);
    console.log(`  -> Saved local SVG: ${localSvg}`);

    try {
      const jpgBytes = await renderJpg(svg, 1440);
      await Bun.write(localJpg, jpgBytes);
      console.log(`  -> Saved local JPG: ${localJpg}`);
    } catch (err) {
      console.warn(`  -> Resvg render JPG warning: ${err.message || err}`);
    }
  }

  console.log("\nLocal SVG & JPG generation complete!");
};

if (import.meta.main) {
  await benchSvgGen();
}

export default benchSvgGen;
