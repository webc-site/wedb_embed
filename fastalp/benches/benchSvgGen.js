#!/usr/bin/env -S bun
import { mkdir, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { $ } from "bun";
import { parse as yamlParse } from "yaml";
import { loadBenchData, getSystemEnv } from "./lib/data.js";
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

    let cdnUrl = "";
    try {
      console.log(`3. Uploading ${lang} SVG to GitHub CDN...`);
      const svgBuf = await readFile(localSvg);
      cdnUrl = await uploadSvg(svgBuf);
      urlMap[lang] = cdnUrl;
      console.log(`CDN URL [${lang}]: ${cdnUrl}`);
    } catch (err) {
      console.warn(`Upload CDN warning: ${err.message || err}`);
      cdnUrl = `https://raw.githubusercontent.com/webc-site/wedb_embed/main/fastalp/benches/img/${lang}/bench.svg`;
    }

    // Update fastalp/readme/{lang}.md
    console.log(`4. Updating fastalp/readme/${lang}.md hero image...`);
    const readmePath = resolve(FASTALP_DIR, `readme/${lang}.md`);
    const file = Bun.file(readmePath);
    if (await file.exists()) {
      let content = await file.text();
      const sysEnv = getSystemEnv(lang === "zh");
      const heroBlock = `<p align="center">\n  <img src="${cdnUrl}" alt="${i18n.hero_alt}" width="100%">\n  <br>\n  <sub><b>${i18n.env_title}</b>: ${sysEnv.cpu} ｜ ${sysEnv.toolchain}</sub>\n</p>`;

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
