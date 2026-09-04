#!/usr/bin/env -S bun
import { resolve } from "node:path";
import { $ } from "bun";
import { loadBenchData } from "./lib/data.js";
import { renderMd } from "./lib/renderMd.js";

const BENCHES_DIR = import.meta.dirname;
const ROOT_DIR = resolve(BENCHES_DIR, "..");
const LANG_LI = ["zh", "en"];

export const benchMdGen = async () => {
  console.log("1. Loading benchmark JSONs...");
  const benchData = await loadBenchData();

  for (const lang of LANG_LI) {
    console.log(`2. Generating readme/${lang}/bench.md from JSON...`);
    const md = renderMd(benchData, lang);
    const targetFile = resolve(ROOT_DIR, `readme/${lang}/bench.md`);
    await Bun.write(targetFile, md);
    console.log(`  -> Saved ${targetFile}`);
  }

  console.log("3. Compiling fastalp/README.md with mdt...");
  try {
    await $`bun x mdt .`.cwd(ROOT_DIR);
    console.log("  -> fastalp/README.md updated successfully!");
  } catch (err) {
    console.warn(`  -> mdt compile warning: ${err.message || err}`);
  }

  console.log("\nBenchmark documentation generation complete!");
};

if (import.meta.main) {
  await benchMdGen();
}

export default benchMdGen;
