import { readFile } from "node:fs/promises";
import { optimize } from "svgo";
import { Resvg } from "@resvg/resvg-js";
import cdnUpload from "@1-/github_cdn";

const FONT_FILE_LI = [
  "/System/Library/Fonts/Hiragino Sans GB.ttc",
  "/System/Library/Fonts/STHeiti Medium.ttc",
  "/System/Library/Fonts/STHeiti Light.ttc",
  "/System/Library/Fonts/Supplemental/Songti.ttc",
  "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
  "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
  "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
  "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
  "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
].filter((f) => Bun.file(f).size > 0);

const token = () => {
  const t = process.env.GH_TOKEN || process.env.GITHUB_TOKEN;
  if (!t) {
    throw new Error("缺少 GH_TOKEN 或 GITHUB_TOKEN 环境变量，无法上传 SVG 至 CDN");
  }
  return t;
};

export const optimizeSvg = (rawSvg) => {
  return optimize(rawSvg, {
    multipass: true,
  }).data;
};

export const renderJpg = async (svgStr, targetWidth = 1440) => {
  const resvg = new Resvg(svgStr, {
    fitTo: { mode: "width", value: targetWidth },
    background: "#070b14",
    font: {
      loadSystemFonts: false,
      fontFiles: FONT_FILE_LI,
      defaultFontFamily: "Hiragino Sans GB",
    },
  });
  const pngBuffer = resvg.render().asPng();
  const img = new Bun.Image(pngBuffer);
  return await img.jpeg({ quality: 95 }).bytes();
};

export const uploadSvg = async (svgBuffer) => {
  const ghToken = token();
  const upload = cdnUpload(ghToken, "webc-fs/-");
  const rawUrl = await upload(svgBuffer, "svg");
  const { pathname } = new URL(rawUrl.startsWith("//") ? "https:" + rawUrl : rawUrl);
  return "https://fastly.jsdelivr.net" + pathname;
};

export default {
  optimizeSvg,
  renderJpg,
  uploadSvg,
};
