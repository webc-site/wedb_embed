#!/usr/bin/env -S bun
import { mkdir, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { $ } from "bun";
import { Resvg } from "@resvg/resvg-js";
import { parse as yamlParse } from "yaml";
import { Eta } from "eta";
import { optimize } from "svgo";
import cdnUpload from "@1-/github_cdn";

const ROOT_DIR = resolve(import.meta.dirname, "../.."),
  BENCH_DATA_JSON = resolve(ROOT_DIR, "embed/benches/benchData.json"),
  I18N_DIR = resolve(import.meta.dirname, "i18n"),
  TMPL_DIR = resolve(import.meta.dirname, "tmpl"),
  ETA = new Eta({ views: TMPL_DIR }),
  LANG_LI = ["zh", "en"];

// 获取 GitHub Token
const token = () => {
  const t = process.env.GH_TOKEN || process.env.GITHUB_TOKEN;
  if (!t) {
    throw new Error("缺少 GH_TOKEN 或 GITHUB_TOKEN 环境变量，无法上传 SVG 至 CDN");
  }
  return t;
};

// 1. 统一以 us 为单位格式化延迟
const latencyFormat = (ns) => {
  if (ns < 1000) return (ns / 1000).toFixed(2) + " us";
  if (ns >= 1000000) return (ns / 1000000).toFixed(2) + " ms";
  return (ns / 1000).toFixed(1) + " us";
};

// 2. 加载指定语言的 YAML 配置文件 (embed/benches/i18n/{lang}.yml)
const i18nLoad = async (lang = "zh") => {
  const yml_path = resolve(I18N_DIR, lang + ".yml"),
    content = await Bun.file(yml_path).text();
  return yamlParse(content);
};

// 3. 生成指定语言的独立结构卡片长图、透明背景、真实全局比例条形图（使用模块化 Eta 模板与 svgo 压缩）
const svgRender = (cmp_data_li, footprint_data, overall_speedup_str, record_data_li = [], config, lang = "zh") => {
  const qpsFormat = (qps_wan) => {
    if (lang === "zh") {
      return qps_wan >= 10 ? Math.round(qps_wan) + " 万/秒" : qps_wan.toFixed(1) + " 万/秒";
    }
    const ops = qps_wan * 10000;
    if (ops >= 1000000) return (ops / 1000000).toFixed(2) + " M ops/s";
    if (ops >= 1000) return (ops / 1000).toFixed(1) + " k ops/s";
    return Math.round(ops) + " ops/s";
  };

  const cmp_map = new Map(cmp_data_li.map((c) => [c.cmd, c])),
    record_map = new Map(record_data_li.map((r) => [r.cmd, r])),
    get_cmd = cmp_map.get("GET") || { redis_ns: 1, wedb_ns: 1 },
    get_speedup = (get_cmd.redis_ns / get_cmd.wedb_ns).toFixed(1) + "x";

  const raw_payload_mb = Math.round(footprint_data.raw_payload_mb),
    wedb_rss_mb = Math.round(footprint_data.wedb.rss_mb),
    redis_rss_mb = Math.round(footprint_data.redis.rss_mb),
    total_items_str = footprint_data.items.toLocaleString(),
    wedb_disk_gb = (
      footprint_data.wedb.disk_gb != null
        ? Number(footprint_data.wedb.disk_gb)
        : footprint_data.wedb.disk_mb / 1024
    ).toFixed(2),
    redis_disk_gb = (
      footprint_data.redis.disk_gb != null
        ? Number(footprint_data.redis.disk_gb)
        : footprint_data.redis.disk_mb / 1024
    ).toFixed(2),
    disk_saved_pct =
      footprint_data.redis.disk_mb > 0
        ? Math.round(
            ((footprint_data.redis.disk_mb - footprint_data.wedb.disk_mb) /
              footprint_data.redis.disk_mb) *
              100,
          ) + "%"
        : "N/A",
    mem_saved_pct =
      footprint_data.redis.rss_mb > 0
        ? Math.round(
            ((footprint_data.redis.rss_mb - footprint_data.wedb.rss_mb) / footprint_data.redis.rss_mb) *
              100,
          ) + "%"
        : "N/A",
    wedb_get_lat_str = latencyFormat(get_cmd.wedb_ns),
    redis_get_lat_str = latencyFormat(get_cmd.redis_ns);

  // 全局最大 QPS（用于对齐全图所有指令的条形长度真实物理比例）
  const all_qps_li = [
    ...cmp_data_li.map((c) => 100000 / c.wedb_ns),
    ...cmp_data_li.map((c) => 100000 / c.redis_ns),
    ...record_data_li.map((r) => 100000 / (r.median_ns || 1000)),
  ],
    max_qps_all = Math.max(...all_qps_li);

  // 渲染单个命令条形图数据对象
  const cmdItemBuild = (cmd_def, rel_y) => {
    const is_exclusive = typeof cmd_def === "object" && cmd_def.exclusive,
      cmd_name = typeof cmd_def === "string" ? cmd_def : cmd_def.cmd,
      desc = is_exclusive ? cmd_def.desc || "" : "";

    if (is_exclusive) {
      const rec = record_map.get(cmd_name) || { median_ns: 1000 },
        wedb_qps_num = 100000 / (rec.median_ns || 1000),
        wedb_qps_str = qpsFormat(wedb_qps_num),
        wedb_lat = latencyFormat(rec.median_ns || 1000);

      const max_w = 210,
        wedb_bar_w = Math.max(
          6,
          Math.min(max_w, Math.round(Math.pow(wedb_qps_num / max_qps_all, 0.65) * max_w)),
        ),
        wedb_in_bar = wedb_bar_w >= 85,
        wedb_label_x = wedb_in_bar ? wedb_bar_w - 6 : wedb_bar_w + 6,
        wedb_label_anchor = wedb_in_bar ? "end" : "start",
        wedb_label_class = wedb_in_bar ? "bar-text" : "bar-text-blue",
        wedb_label_text = wedb_qps_str + " (" + wedb_lat + ")";

      return {
        rel_y,
        is_exclusive: true,
        cmd: cmd_name,
        desc,
        wedb_bar_w,
        wedb_label_x,
        wedb_label_anchor,
        wedb_label_class,
        wedb_label_text,
      };
    }

    const cmd_info = cmp_map.get(cmd_name);
    if (!cmd_info) return null;

    const ratio = cmd_info.redis_ns / cmd_info.wedb_ns,
      speedup = ratio.toFixed(1) + "x",
      wedb_qps_num = 100000 / cmd_info.wedb_ns,
      redis_qps_num = 100000 / cmd_info.redis_ns,
      wedb_qps_str = qpsFormat(wedb_qps_num),
      redis_qps_str = qpsFormat(redis_qps_num),
      wedb_lat = latencyFormat(cmd_info.wedb_ns),
      redis_lat = latencyFormat(cmd_info.redis_ns);

    const max_w = 210,
      wedb_bar_w = Math.max(
        6,
        Math.min(max_w, Math.round(Math.pow(wedb_qps_num / max_qps_all, 0.65) * max_w)),
      ),
      redis_bar_w = Math.max(
        4,
        Math.min(max_w, Math.round(Math.pow(redis_qps_num / max_qps_all, 0.65) * max_w)),
      ),
      wedb_in_bar = wedb_bar_w >= 85,
      wedb_label_x = wedb_in_bar ? wedb_bar_w - 6 : wedb_bar_w + 6,
      wedb_label_anchor = wedb_in_bar ? "end" : "start",
      wedb_label_class = wedb_in_bar ? "bar-text" : "bar-text-blue",
      wedb_label_text = wedb_qps_str + " (" + wedb_lat + ")",
      redis_label_x = redis_bar_w + 6,
      redis_label_anchor = "start",
      redis_label_class = "bar-text-muted",
      redis_label_text = redis_qps_str + " (" + redis_lat + ")";

    return {
      rel_y,
      is_exclusive: false,
      cmd: cmd_name,
      speedup,
      wedb_bar_w,
      wedb_label_x,
      wedb_label_anchor,
      wedb_label_class,
      wedb_label_text,
      redis_bar_w,
      redis_label_x,
      redis_label_anchor,
      redis_label_class,
      redis_label_text,
    };
  };

  const speedup_num = parseFloat(overall_speedup_str) || 40.7,
    card1_wedb_bar_w = 92,
    card1_redis_bar_w = Math.max(5, Math.min(85, Math.round(92 / speedup_num))),
    max_get_lat = Math.max(get_cmd.redis_ns, get_cmd.wedb_ns, 1),
    card2_redis_bar_w = 92,
    card2_wedb_bar_w = Math.max(5, Math.min(85, Math.round((get_cmd.wedb_ns / max_get_lat) * 92))),
    card3_max_disk = Math.max(Number(redis_disk_gb), Number(wedb_disk_gb), 1),
    card3_wedb_bar_w = Math.max(8, Math.round((Number(wedb_disk_gb) / card3_max_disk) * 92)),
    card3_redis_bar_w = Math.max(8, Math.round((Number(redis_disk_gb) / card3_max_disk) * 92)),
    card4_max_rss = Math.max(redis_rss_mb, wedb_rss_mb, 1),
    card4_wedb_bar_w = Math.max(8, Math.round((wedb_rss_mb / card4_max_rss) * 92)),
    card4_redis_bar_w = Math.max(8, Math.round((redis_rss_mb / card4_max_rss) * 92));

  const columnCardsBuild = (cards_conf) => {
    let current_y = 298;
    const card_li = [];
    for (const card_def of (cards_conf || [])) {
      const card_y = current_y;
      let rel_y = 52;
      const item_li = [];
      for (const cmd_def of card_def.cmds) {
        const item = cmdItemBuild(cmd_def, rel_y);
        if (!item) continue;
        item_li.push(item);
        rel_y += item.is_exclusive ? 32 : 39;
      }
      const card_height = rel_y + 4;
      current_y += card_height + 12;
      card_li.push({
        title: card_def.name,
        scene: card_def.scene,
        count_str: item_li.length + " ops",
        y: card_y,
        height: card_height,
        item_li,
      });
    }
    return { card_li, bottom_y: current_y };
  };

  const col1_res = columnCardsBuild(config.col1),
    col2_res = columnCardsBuild(config.col2),
    max_bottom_y = Math.max(col1_res.bottom_y, col2_res.bottom_y),
    total_h = max_bottom_y + 16,
    col_li = [
      { x: 12, card_li: col1_res.card_li },
      { x: 366, card_li: col2_res.card_li },
    ];

  const wedb_avg_ops = cmp_data_li.reduce((acc, c) => acc + (1000000000 / c.wedb_ns), 0) / (cmp_data_li.length || 1),
    redis_avg_ops = cmp_data_li.reduce((acc, c) => acc + (1000000000 / c.redis_ns), 0) / (cmp_data_li.length || 1),
    wedb_avg_qps_str = lang === "zh"
      ? (wedb_avg_ops / 10000).toFixed(2) + " 万/秒"
      : (wedb_avg_ops >= 1000000 ? (wedb_avg_ops / 1000000).toFixed(2) + " M ops/s" : (wedb_avg_ops / 1000).toFixed(1) + " k ops/s"),
    redis_avg_qps_str = lang === "zh"
      ? (redis_avg_ops / 10000).toFixed(2) + " 万/秒"
      : (redis_avg_ops >= 1000000 ? (redis_avg_ops / 1000000).toFixed(2) + " M ops/s" : (redis_avg_ops / 1000).toFixed(1) + " k ops/s"),
    subtitle = config.subtitle
      .replace("{items}", total_items_str)
      .replace("{payload}", raw_payload_mb),
    card1_num = config.card1_num.replace("{speedup}", overall_speedup_str),
    card1_sub = config.card1_sub
      .replace("{wedb_ops}", wedb_avg_qps_str)
      .replace("{redis_ops}", redis_avg_qps_str),
    card2_num = config.card2_num.replace("{speedup}", get_speedup),
    card2_sub = config.card2_sub
      .replace("{wedb_lat}", wedb_get_lat_str)
      .replace("{redis_lat}", redis_get_lat_str),
    card3_num = config.card3_num.replace("{pct}", disk_saved_pct),
    card3_sub = config.card3_sub
      .replace("{wedb_disk}", wedb_disk_gb)
      .replace("{redis_disk}", redis_disk_gb),
    card4_num = config.card4_num.replace("{pct}", mem_saved_pct),
    card4_sub = config.card4_sub
      .replace("{wedb_rss}", wedb_rss_mb)
      .replace("{redis_rss}", redis_rss_mb);

  const rendered = ETA.render("./svg.eta", {
    title: config.title,
    subtitle,
    card1_title: config.card1_title,
    card1_num,
    card1_sub,
    wedb_avg_qps_str,
    redis_avg_qps_str,
    card1_wedb_bar_w,
    card1_redis_bar_w,
    card2_title: config.card2_title,
    card2_num,
    card2_sub,
    wedb_get_lat_str,
    redis_get_lat_str,
    card2_wedb_bar_w,
    card2_redis_bar_w,
    card3_title: config.card3_title,
    card3_num,
    card3_sub,
    wedb_disk_gb,
    redis_disk_gb,
    card3_wedb_bar_w,
    card3_redis_bar_w,
    card4_title: config.card4_title,
    card4_num,
    card4_sub,
    wedb_rss_mb,
    redis_rss_mb,
    card4_wedb_bar_w,
    card4_redis_bar_w,
    sec_title: config.sec_title,
    sec_sub: config.sec_sub,
    col_li,
    total_h,
  });

  return optimize(rendered, { multipass: true }).data;
};

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

// 4. 双语 SVG & JPG (压缩率 95) 生成至 embed/benches/img/{lang}/ 并上传至 GitHub CDN 仓库 (webc-fs/-)
const svgGenerate = async (cmp_data_li, footprint_data, overall_speedup_str, record_data_li = []) => {
  const gh_token = token(),
    upload = cdnUpload(gh_token, "webc-fs/-"),
    tmp_dir = join(tmpdir(), "wedb_bench_svg_" + Date.now());

  await mkdir(tmp_dir, { recursive: true });
  try {
    const url_li = await Promise.all(
      LANG_LI.map(async (lang) => {
        const config = await i18nLoad(lang),
          svg = svgRender(cmp_data_li, footprint_data, overall_speedup_str, record_data_li, config, lang),
          file_path = join(tmp_dir, lang + ".svg"),
          out_dir = resolve(ROOT_DIR, "embed/benches/img/" + lang),
          local_svg = resolve(out_dir, "bench.svg"),
          local_jpg = resolve(out_dir, "bench.jpg");

        await mkdir(out_dir, { recursive: true });
        await Bun.write(file_path, svg);
        await Bun.write(local_svg, svg);

        // 渲染生成 JPG（2x 高清超采样，压缩率 95，精准加载中文字体，本地渲染不上传 CDN）
        const resvg = new Resvg(svg, {
          fitTo: { mode: "width", value: 1440 },
          background: "#ffffff",
          font: {
            loadSystemFonts: false,
            fontFiles: FONT_FILE_LI,
            defaultFontFamily: "Hiragino Sans GB",
          },
        });
        const png_buffer = resvg.render().asPng(),
          img = new Bun.Image(png_buffer),
          jpg_bytes = await img.jpeg({ quality: 95 }).bytes();
        await Bun.write(local_jpg, jpg_bytes);

        const buf = await readFile(file_path),
          url = await upload(buf, "svg"),
          { pathname } = new URL(url.startsWith("//") ? "https:" + url : url);
        return "https://fastly.jsdelivr.net" + pathname;
      }),
    );

    url_li.forEach((url) => console.log(url));
    return url_li;
  } finally {
    await rm(tmp_dir, { force: true, recursive: true }).catch(() => {});
  }
};

// 5. 更新 Markdown 报告与模板渲染
const markdownUpdate = async (bench_data, url_li) => {
  const { env, footprint, cmp_li, record_li } = bench_data,
    BENCH_DOC_DIR = resolve(ROOT_DIR, "embed/readme/bench");

  await mkdir(BENCH_DOC_DIR, { recursive: true });

  const raw_payload_mb = Math.round(footprint.raw_payload_mb),
    wedb_disk_mb = Math.round(footprint.wedb.disk_mb),
    wedb_rss_mb = Math.round(footprint.wedb.rss_mb),
    redis_disk_mb = Math.round(footprint.redis.disk_mb),
    redis_rss_mb = Math.round(footprint.redis.rss_mb),
    total_items_str = footprint.items.toLocaleString(),
    disk_saved_pct =
      footprint.redis.disk_mb > 0
        ? Math.round(
            ((footprint.redis.disk_mb - footprint.wedb.disk_mb) / footprint.redis.disk_mb) * 100,
          ) + "%"
        : "N/A",
    mem_saved_pct =
      footprint.redis.rss_mb > 0
        ? Math.round(
            ((footprint.redis.rss_mb - footprint.wedb.rss_mb) / footprint.redis.rss_mb) * 100,
          ) + "%"
        : "N/A";

  const formatStr = (template, vars) =>
    template.replace(/\{(\w+)\}/g, (_, k) => (vars[k] !== undefined ? vars[k] : ""));

  const renderMdForLang = async (lang) => {
    const i18n_path = resolve(I18N_DIR, `${lang}.yml`),
      i18n_raw = await readFile(i18n_path, "utf-8"),
      i18n = yamlParse(i18n_raw),
      r = i18n.report;

    const vars = {
      cpu_model: env.cpu_model,
      cpu_cores: env.cpu_cores,
      total_mem_gb: env.total_mem_gb,
      disk_info: env.disk_info || "NVMe SSD",
      os_name: env.os_name,
      rust_ver: env.rust_ver,
      redis_ver: env.redis_ver,
      items: total_items_str,
      payload_mb: raw_payload_mb,
      payload_gb: raw_payload_mb >= 1024 ? (raw_payload_mb / 1024).toFixed(1) + " GB" : raw_payload_mb + " MB",
      wedb_disk_mb,
      redis_disk_mb,
      disk_saved_pct,
      wedb_rss_mb,
      redis_rss_mb,
      mem_saved_pct,
    };

    const hw_info = `
${formatStr(r.hw_cpu, vars)}<br>
${formatStr(r.hw_mem, vars)}<br>
${formatStr(r.hw_disk, vars)}<br>
${formatStr(r.hw_os, vars)}<br>
${formatStr(r.hw_rust, vars)}<br>
${formatStr(r.hw_redis, vars)}
`;

    let cmp_table = `| ${r.cmp_col_cmd} | ${r.cmp_col_wedb_p95} | ${r.cmp_col_redis_p95} | ${r.cmp_col_speedup} |\n| :--- | :--- | :--- | :--- |\n`;
    cmp_li.forEach((c) => {
      cmp_table += `| \`${c.cmd}\` | ${latencyFormat(c.wedb_ns)} | ${latencyFormat(c.redis_ns)} | **${c.speedup}** |\n`;
    });

    const md = `### ${lang === "zh" ? env.title_zh : env.title_en}

#### ${r.hw_title}
${hw_info}
#### ${formatStr(r.footprint_title, vars)}

| ${r.footprint_col_dim} | ${formatStr(r.footprint_col_wedb, vars)} | ${formatStr(r.footprint_col_redis, vars)} | ${r.footprint_col_saved} |
| :--- | :--- | :--- | :--- |
| ${r.footprint_row_items} | ${formatStr(r.footprint_val_items, vars)} | ${formatStr(r.footprint_val_items, vars)} | ${r.footprint_desc_items} |
| ${r.footprint_row_payload} | ${formatStr(r.footprint_val_payload, vars)} | ${formatStr(r.footprint_val_payload, vars)} | ${r.footprint_desc_payload} |
| ${r.footprint_row_disk} | ${formatStr(r.footprint_val_disk, { disk_mb: wedb_disk_mb })} | ${formatStr(r.footprint_val_disk, { disk_mb: redis_disk_mb })} | ${formatStr(r.footprint_desc_disk, { pct: disk_saved_pct })} |
| ${r.footprint_row_rss} | ${formatStr(r.footprint_val_rss, { rss_mb: wedb_rss_mb })} | ${formatStr(r.footprint_val_rss, { rss_mb: redis_rss_mb })} | ${formatStr(r.footprint_desc_rss, { pct: mem_saved_pct })} |

#### ${r.cmp_title}

${cmp_table}
`;
    return { md, i18n, vars };
  };

  await Promise.all(
    LANG_LI.map(async (lang, idx) => {
      const { md, i18n, vars } = await renderMdForLang(lang);
      await Promise.all([
        Bun.write(resolve(BENCH_DOC_DIR, env.slug + "_" + lang + ".md"), md),
        Bun.write(resolve(BENCH_DOC_DIR, lang + ".md"), md),
      ]);

      const file_path = resolve(ROOT_DIR, "embed/readme/" + lang + ".md"),
        url = url_li[idx],
        r = i18n.report;
      try {
        const file = Bun.file(file_path);
        if (!(await file.exists())) return;
        const content = await file.text(),
          hero_block = `<p align="center">\n  <img src="${url}" alt="${r.hero_alt}" width="100%">\n  <br>\n  <sub><b>${r.hero_env}</b>: ${formatStr(r.hw_cpu, vars)} ｜ ${formatStr(r.hw_mem, vars)} ｜ ${formatStr(r.hw_disk, vars)} ｜ ${formatStr(r.hw_os, vars)} ｜ ${formatStr(r.hw_rust, vars)} ｜ ${formatStr(r.hw_redis, vars)}</sub>\n</p>`;

        const updated = content.replace(
          /<p align="center">\s*<img src="[^"]+"[^>]*alt="wedb_embed vs Redis[^"]*"[^>]*>(?:[\s\S]*?<\/sub>)?\s*<\/p>/,
          hero_block,
        );
        if (updated !== content) {
          await Bun.write(file_path, updated);
        }
      } catch (err) {
        console.warn("heroUpdate " + file_path + ":", err.message || err);
      }
    }),
  );
};

// 6. 执行 mdt 模板编译
const mdtCompile = async () => {
  try {
    await $`bun x @1-/mdt`.cwd(ROOT_DIR).quiet();
  } catch (err) {
    console.warn("mdt:", err.message || err);
  }
};

export const benchSvgGen = async (bench_data) => {
  const data = bench_data || (await Bun.file(BENCH_DATA_JSON).json()),
    url_li = await svgGenerate(data.cmp_li, data.footprint, data.overall_speedup, data.record_li);
  await markdownUpdate(data, url_li);
  await mdtCompile();
  return url_li;
};

if (import.meta.main) {
  await benchSvgGen();
}

export default benchSvgGen;
