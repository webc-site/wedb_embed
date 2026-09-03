#!/usr/bin/env -S bun
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { $ } from "bun";
import { parse as yamlParse } from "yaml";
import { loadBenchData } from "./lib/data.js";
import { renderSvg } from "./lib/render.js";
import { optimizeSvg, renderJpg, uploadSvg } from "./lib/upload.js";

const BENCHES_DIR = import.meta.dirname;
const ROOT_DIR = resolve(BENCHES_DIR, "../..");
const FASTALP_DIR = resolve(BENCHES_DIR, "..");
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

  const urlMap = {};

  for (const lang of LANG_LI) {
    console.log(`\n2. Generating SVG and JPG for [${lang}]...`);
    const i18n = await i18nLoad(lang);
    const rawSvg = renderSvg(benchData, i18n, lang);
    const svg = optimizeSvg(rawSvg);

    const outDir = resolve(IMG_DIR, lang);
    await mkdir(outDir, { recursive: true });

    const localSvg = resolve(outDir, "bench.svg");
    const localJpg = resolve(outDir, "bench.jpg");

    await Bun.write(localSvg, svg);
    console.log(`Saved local SVG: ${localSvg}`);

    try {
      const jpgBytes = await renderJpg(svg, 1440);
      await Bun.write(localJpg, jpgBytes);
      console.log(`Saved local JPG: ${localJpg}`);
    } catch (err) {
      console.warn(`Resvg render JPG warning: ${err.message || err}`);
    }

    console.log(`3. Uploading ${lang} SVG to GitHub CDN...`);
    const svgBuf = await readFile(localSvg);
    const cdnUrl = await uploadSvg(svgBuf);
    urlMap[lang] = cdnUrl;
    console.log(`CDN URL [${lang}]: ${cdnUrl}`);

    // Update fastalp/readme/{lang}.md
    console.log(`4. Updating fastalp/readme/${lang}.md hero image...`);
    const readmePath = resolve(FASTALP_DIR, `readme/${lang}.md`);
    const file = Bun.file(readmePath);
    if (await file.exists()) {
      let content = await file.text();
      const linkTitle = lang === "zh" ? "🔗 SVG 高清矢量原图直链" : "🔗 Vector SVG Direct Link";
      const heroBlock = `<p align="center">\n  <a href="${cdnUrl}" target="_blank">\n    <img src="${cdnUrl}" alt="${i18n.hero_alt}" width="100%">\n  </a>\n  <br>\n  <b>${linkTitle}</b>: <a href="${cdnUrl}"><code>${cdnUrl}</code></a>\n  <br><br>\n  <sub><b>${i18n.env_title}</b>: ${i18n.env_cpu} ｜ ${i18n.env_os} ｜ ${i18n.env_toolchain}</sub>\n</p>`;

      const heroRegex = /<p align="center">[\s\S]*?alt="fastalp[^"]*"[\s\S]*?<\/p>/;
      if (heroRegex.test(content)) {
        content = content.replace(heroRegex, heroBlock);
      } else {
        // Insert after the introductory paragraph
        const marker = "\n\n---\n";
        const idx = content.indexOf(marker);
        if (idx !== -1) {
          content = content.slice(0, idx) + `\n\n${heroBlock}` + content.slice(idx);
        }
      }

      await Bun.write(readmePath, content);
      console.log(`Updated ${readmePath}`);
    }
  }

  // Compile mdt to update fastalp/README.md
  console.log("\n5. Compiling fastalp/README.md with mdt...");
  try {
    await $`npx mdt fastalp`.cwd(ROOT_DIR);
    console.log("Compiled fastalp/README.md successfully!");
  } catch (err) {
    console.warn(`mdt compile warning: ${err.message || err}`);
  }

  console.log("\nAll done!");
  return urlMap;
};

if (import.meta.main) {
  await benchSvgGen();
}

export default benchSvgGen;
