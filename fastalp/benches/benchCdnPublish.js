#!/usr/bin/env -S bun
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { $ } from "bun";
import { parse as yamlParse } from "yaml";
import { getSystemEnv } from "./lib/data.js";
import { uploadSvg } from "./lib/upload.js";

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

export const benchCdnPublish = async () => {
  const urlMap = {};

  for (const lang of LANG_LI) {
    const i18n = await i18nLoad(lang);
    const localSvg = resolve(IMG_DIR, `${lang}/bench.svg`);
    const file = Bun.file(localSvg);
    if (!(await file.exists())) {
      console.error(`Error: ${localSvg} does not exist. Run benches/benchSvgGen.js first!`);
      continue;
    }

    let cdnUrl = "";
    try {
      console.log(`\n1. Uploading ${lang} SVG to GitHub CDN...`);
      const svgBuf = await readFile(localSvg);
      cdnUrl = await uploadSvg(svgBuf);
      urlMap[lang] = cdnUrl;
      console.log(`  -> CDN URL [${lang}]: ${cdnUrl}`);
    } catch (err) {
      console.warn(`  -> Upload CDN warning: ${err.message || err}`);
      cdnUrl = `https://raw.githubusercontent.com/webc-site/wedb_embed/main/fastalp/benches/img/${lang}/bench.svg`;
    }

    // Update fastalp/readme/{lang}/intro.md
    console.log(`2. Updating fastalp/readme/${lang}/intro.md hero image...`);
    const readmePath = resolve(FASTALP_DIR, `readme/${lang}/intro.md`);
    const mdFile = Bun.file(readmePath);
    if (await mdFile.exists()) {
      let content = await mdFile.text();
      const sysEnv = getSystemEnv(lang === "zh");
      const heroBlock = `<p align="center">\n  <img src="${cdnUrl}" alt="${i18n.hero_alt}" width="100%">\n  <br>\n  <sub><b>${i18n.env_title}</b>: ${sysEnv.cpu} ｜ ${sysEnv.toolchain}</sub>\n</p>`;

      const heroRegex = /<p align="center">[\s\S]*?alt="fastalp[^"]*"[\s\S]*?<\/p>/;
      if (heroRegex.test(content)) {
        content = content.replace(heroRegex, heroBlock);
      } else {
        const marker = "\n\n---\n";
        const idx = content.indexOf(marker);
        if (idx !== -1) {
          content = content.slice(0, idx) + `\n\n${heroBlock}` + content.slice(idx);
        }
      }

      await Bun.write(readmePath, content);
      console.log(`  -> Updated ${readmePath}`);
    }
  }

  // Compile mdt to update fastalp/README.md
  console.log("\n3. Compiling fastalp/README.md with mdt...");
  try {
    await $`bun x mdt fastalp`.cwd(ROOT_DIR);
    console.log("  -> Compiled fastalp/README.md successfully!");
  } catch (err) {
    console.warn(`  -> mdt compile warning: ${err.message || err}`);
  }

  console.log("\nAll CDN publish and markdown sync done!");
  return urlMap;
};

if (import.meta.main) {
  await benchCdnPublish();
}

export default benchCdnPublish;
