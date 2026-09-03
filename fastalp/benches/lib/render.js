import { getSystemEnv, computeScenarioMetrics } from "./data.js";

const xmlEscape = (str) => {
  if (typeof str !== "string") return str ?? "";
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
};

export const renderSvg = (benchData, rawI18n, lang = "zh") => {
  const { algorithms } = benchData;
  const isZh = lang === "zh";
  const sysEnv = getSystemEnv(isZh);

  const i18n = Object.fromEntries(
    Object.entries(rawI18n).map(([k, v]) => [k, typeof v === "string" ? xmlEscape(v) : v])
  );

  const width = 1240;

  // Header geometry: Title block and Environment Box are exactly the same height (78px)
  const topPad = 36;
  const headerBoxH = 78;

  // Unified Section Gap: exactly 36px between Section 1 & 2
  const SECTION_GAP = 36;

  // Section 1: Main Table (8 Codecs Overview) starts directly below Header Box
  const sec1Y = topPad + headerBoxH + 32;
  const sec1HeaderY = sec1Y + 36;
  const sec1HeaderH = 34;
  const sec1FirstRowY = sec1HeaderY + sec1HeaderH + 12; // 12px margin below header
  const sec1RowH = 46;
  const sec1TableBottom = sec1FirstRowY + (algorithms.length - 1) * sec1RowH + 42;

  // Section 2: 6 Industrial Scenario Cards (3x2 Grid)
  const sec2Y = sec1TableBottom + SECTION_GAP;
  const scW = (width - 64 - 24) / 2; // 576px wide each card
  const scH = 228;
  const sec2CardsY = sec2Y + 46; // Subtitle is at sec2Y + 32, cards start at sec2Y + 46 (gap = 14px)

  const sec2Bottom = sec2CardsY + 3 * scH + 2 * 16;
  const footerLines = (i18n.footer_lines || [i18n.footer_left]).map((line) =>
    line.replace("{cpu}", sysEnv.cpuModel || "Apple Silicon")
  );
  const footerLineH = 20;
  const footerY = sec2Bottom + 28;
  const totalH = footerY + (footerLines.length - 1) * footerLineH + 32;

  // Palette: Cool Sapphire Blue for Decompression, Warm Amber Gold for Compression
  // Palette: Cool Sapphire Blue for Compression, Vibrant Orange-Red for Decompression
  const barColors = {
    fastalp: { dec: "#ea580c", enc: "#2563eb", text: "#c2410c", encText: "#1d4ed8" },
    cpp_alp: { dec: "#f97316", enc: "#3b82f6", text: "#9a3412", encText: "#1e3a8a" },
    pco:     { dec: "#64748b", enc: "#64748b", text: "#475569", encText: "#475569" },
    zstd:    { dec: "#64748b", enc: "#64748b", text: "#475569", encText: "#475569" },
    lz4:     { dec: "#64748b", enc: "#64748b", text: "#475569", encText: "#475569" },
    snappy:  { dec: "#64748b", enc: "#64748b", text: "#475569", encText: "#475569" },
    chimp128:{ dec: "#64748b", enc: "#64748b", text: "#475569", encText: "#475569" },
    gorilla: { dec: "#64748b", enc: "#64748b", text: "#475569", encText: "#475569" },
  };

  const maxDecSpeed = 24.0;
  const maxEncSpeed = 6.0;
  const barW = 85;

  // 1. Render Section 1 rows (8 Codecs) - using Geometric Mean (几何均值)
  const sec1RowsSvg = algorithms.map((algo, idx) => {
    const y = sec1FirstRowY + idx * sec1RowH;
    const isFastalp = algo.algorithm === "fastalp";
    const isCpp = algo.algorithm === "cpp_alp";
    const c = barColors[algo.algorithm] || barColors.gorilla;

    const decSpeed = algo.paper_31.geomean_dec_gb_s || algo.paper_31.avg_dec_gb_s;
    const encSpeed = algo.paper_31.geomean_enc_gb_s || algo.paper_31.avg_enc_gb_s;
    const decBarLen = Math.max(6, Math.min(barW, (decSpeed / maxDecSpeed) * barW));
    const encBarLen = Math.max(6, Math.min(barW, (encSpeed / maxEncSpeed) * barW));

    const ratio = algo.paper_31.geomean_ratio || algo.paper_31.ratio;

    const rowBg = isFastalp
      ? `<rect x="32" y="${y - 6}" width="${width - 64}" height="${sec1RowH - 4}" rx="8" fill="#f0f7ff" stroke="#bfdbfe" stroke-width="1.2"/>`
      : idx % 2 === 1
      ? `<rect x="32" y="${y - 6}" width="${width - 64}" height="${sec1RowH - 4}" rx="8" fill="#f8fafc"/>`
      : "";

    const badgeW = isZh ? 44 : 50;
    const highlightBadge = isFastalp
      ? `<rect x="150" y="${y - 2}" width="${badgeW}" height="16" rx="3" fill="#2563eb"/><text x="${150 + badgeW / 2}" y="${y + 10}" font-size="9" font-weight="bold" fill="#ffffff" text-anchor="middle">${i18n.badge_leader}</text>`
      : isCpp
      ? `<rect x="195" y="${y - 2}" width="${badgeW}" height="16" rx="3" fill="#e2e8f0"/><text x="${195 + badgeW / 2}" y="${y + 10}" font-size="9" font-weight="600" fill="#475569" text-anchor="middle">${i18n.baseline_text}</text>`
      : "";

    const typeLabel = algo.category === "specialized_float" ? i18n.type_specialized : i18n.type_general;
    const typeColor = algo.category === "specialized_float" ? "#0369a1" : "#64748b";

    const cppAlgo = algorithms.find((a) => a.algorithm === "cpp_alp") || algorithms[1];
    const cppDec = cppAlgo.paper_31.geomean_dec_gb_s || cppAlgo.paper_31.avg_dec_gb_s || 19.8;
    const cppEnc = cppAlgo.paper_31.geomean_enc_gb_s || cppAlgo.paper_31.avg_enc_gb_s || 0.88;

    let decVs = "";
    if (isFastalp) {
      const mult = (decSpeed / cppDec).toFixed(1);
      decVs = isZh ? `较 C++ 快 ${mult}x` : `${mult}x vs C++`;
    } else if (isCpp) {
      decVs = i18n.baseline_text;
    } else {
      const speedup = decSpeed >= cppDec 
        ? (decSpeed / cppDec).toFixed(2) + "x" 
        : "-" + (100 - (decSpeed / cppDec) * 100).toFixed(0) + "%";
      decVs = speedup;
    }

    let encVs = "";
    if (isFastalp) {
      const mult = (encSpeed / cppEnc).toFixed(1);
      encVs = isZh ? `较 C++ 快 ${mult}x` : `${mult}x vs C++`;
    } else if (isCpp) {
      encVs = i18n.baseline_text;
    } else {
      const speedup = (encSpeed / cppEnc).toFixed(1) + "x";
      encVs = speedup;
    }

    return `
    <g class="algo-row">
      ${rowBg}
      <!-- Line 1: Algorithm Name + Badge -->
      <text x="48" y="${y + 11}" font-size="13" font-weight="${isFastalp ? "bold" : "600"}" fill="${isFastalp ? "#1d4ed8" : "#0f172a"}">${xmlEscape(algo.display_name)}</text>
      ${highlightBadge}

      <!-- Line 2: Classification Type with newline below algorithm name -->
      <text x="48" y="${y + 26}" font-size="10" font-weight="500" fill="${typeColor}">${typeLabel}</text>

      <!-- Decompress Throughput -->
      <rect x="280" y="${y + 6}" width="${decBarLen}" height="12" rx="3" fill="${c.dec}"/>
      <text x="${280 + decBarLen + 8}" y="${y + 16}" font-size="12" font-weight="bold" fill="${c.text}">${decSpeed.toFixed(1)} GB/s</text>

      <!-- Decompress vs Baseline -->
      <text x="480" y="${y + 16}" font-size="11.5" font-weight="${isFastalp ? "bold" : "500"}" fill="${isFastalp ? "#c2410c" : "#475569"}">${decVs}</text>

      <!-- Compress Throughput -->
      <rect x="635" y="${y + 6}" width="${encBarLen}" height="12" rx="3" fill="${c.enc}"/>
      <text x="${635 + encBarLen + 8}" y="${y + 16}" font-size="12" font-weight="bold" fill="${c.encText}">${encSpeed.toFixed(1)} GB/s</text>

      <!-- Compress vs Baseline -->
      <text x="830" y="${y + 16}" font-size="11.5" font-weight="${isFastalp ? "bold" : "500"}" fill="${isFastalp ? "#1d4ed8" : "#475569"}">${encVs}</text>

      <!-- Compression Ratio -->
      <text x="985" y="${y + 16}" font-size="13" font-weight="bold" fill="${isFastalp ? "#1d4ed8" : "#0f172a"}">${ratio.toFixed(2)}x</text>
    </g>
    `;
  }).join("\n");

  // 2. Section 2: 6 Scenarios Microbenchmarks (100% Dynamically Computed from Real Datasets)
  const buildScenarioItems = (sceneKey) => {
    const fourthAlgoMap = {
      scene_sensor: { id: "lz4", name: "LZ4" },
      scene_ramp: { id: "zstd", name: "Zstd (Level 3)" },
      scene_finance: { id: "snappy", name: "Snappy (snap)" },
      scene_steady: { id: "zstd", name: "Zstd (Level 3)" },
      scene_geo: { id: "snappy", name: "Snappy (snap)" },
      scene_macro: { id: "zstd", name: "Zstd (Level 3)" },
    };

    const fourth = fourthAlgoMap[sceneKey] || { id: "zstd", name: "Zstd" };
    const list = [
      { id: "fastalp", name: "fastalp (Rust)", isFastalp: true },
      { id: "cpp_alp", name: "C++ ALP", isCpp: true },
      { id: "pco", name: "Pcodec (pco)" },
      fourth
    ];

    const cppAlgoObj = algorithms.find(a => a.algorithm === "cpp_alp") || algorithms[1];
    const cppScene = computeScenarioMetrics(cppAlgoObj, sceneKey);

    return list.map(item => {
      const algoObj = algorithms.find(a => a.algorithm === item.id);
      const sc = computeScenarioMetrics(algoObj, sceneKey);
      const dec = (sc.dec_gb_s || 0).toFixed(sc.dec_gb_s >= 10 ? 1 : 2) + " GB/s";
      const enc = (sc.enc_gb_s || 0).toFixed(1) + " GB/s";
      const ratio = (sc.ratio || 0).toFixed(sc.ratio >= 10 ? 1 : 2) + "x";

      let vs = "";
      if (item.isFastalp) {
        if (sc.ratio > cppScene.ratio * 1.5) {
          const mult = (sc.ratio / cppScene.ratio).toFixed(0);
          vs = isZh ? `比 C++ 高 ${mult}x` : `${mult}x vs C++`;
        } else if (sc.dec_gb_s > cppScene.dec_gb_s) {
          const speedup = (((sc.dec_gb_s / cppScene.dec_gb_s) - 1) * 100).toFixed(0);
          vs = isZh ? `比 C++ 快 ${speedup}%` : `+${speedup}% vs C++`;
        } else if (sc.enc_gb_s > cppScene.enc_gb_s) {
          const speedup = (((sc.enc_gb_s / cppScene.enc_gb_s) - 1) * 100).toFixed(0);
          vs = isZh ? `压缩快 ${speedup}%` : `+${speedup}% vs C++`;
        } else {
          vs = isZh ? "领先 (零损)" : "Leader";
        }
      } else if (item.isCpp) {
        vs = isZh ? "基准" : "Baseline";
      } else {
        const decRatio = cppScene.dec_gb_s > 0 ? (sc.dec_gb_s / cppScene.dec_gb_s).toFixed(2) : "1.00";
        vs = isZh ? `解压 ${decRatio}x` : `${decRatio}x Dec`;
      }

      return {
        name: item.name,
        dec,
        enc,
        ratio,
        vs,
        isFastalp: !!item.isFastalp,
      };
    });
  };

  const scenariosData = [
    {
      title: i18n.scene_sensor_title,
      sub: i18n.scene_sensor_sub,
      badge: i18n.scene_sensor_badge,
      badgeW: isZh ? 112 : 124,
      items: buildScenarioItems("scene_sensor")
    },
    {
      title: i18n.scene_ramp_title,
      sub: i18n.scene_ramp_sub,
      badge: i18n.scene_ramp_badge,
      badgeW: isZh ? 112 : 124,
      items: buildScenarioItems("scene_ramp")
    },
    {
      title: i18n.scene_stock_title,
      sub: i18n.scene_stock_sub,
      badge: i18n.scene_stock_badge,
      badgeW: isZh ? 112 : 124,
      items: buildScenarioItems("scene_finance")
    },
    {
      title: i18n.scene_const_title,
      sub: i18n.scene_const_sub,
      badge: i18n.scene_const_badge,
      badgeW: isZh ? 112 : 124,
      items: buildScenarioItems("scene_steady")
    },
    {
      title: i18n.scene_geo_title,
      sub: i18n.scene_geo_sub,
      badge: i18n.scene_geo_badge,
      badgeW: isZh ? 112 : 124,
      items: buildScenarioItems("scene_geo")
    },
    {
      title: i18n.scene_macro_title,
      sub: i18n.scene_macro_sub,
      badge: i18n.scene_macro_badge,
      badgeW: isZh ? 112 : 124,
      items: buildScenarioItems("scene_macro")
    }
  ];

  const sec2CardsSvg = scenariosData.map((sc, sIdx) => {
    const row = Math.floor(sIdx / 2);
    const col = sIdx % 2;
    const sx = 32 + col * (scW + 24);
    const sy = sec2CardsY + row * (scH + 16);

    const subHeaderY = sy + 48 + 10;
    const subHeaderH = 26;
    const rowsStartY = subHeaderY + subHeaderH + 10;

    const rows = sc.items.map((it, itIdx) => {
      const iy = rowsStartY + 14 + itIdx * 30;
      const isHeader = it.isFastalp;
      const itemBg = isHeader
        ? `<rect x="${sx + 10}" y="${iy - 14}" width="${scW - 20}" height="28" rx="5" fill="#f0f7ff" stroke="#bfdbfe" stroke-width="1"/>`
        : itIdx % 2 === 1
        ? `<rect x="${sx + 10}" y="${iy - 14}" width="${scW - 20}" height="28" rx="5" fill="#f8fafc"/>`
        : "";

      const decColor = isHeader ? "#c2410c" : "#475569";
      const encColor = isHeader ? "#1d4ed8" : "#475569";
      const ratioColor = isHeader ? "#0f172a" : "#475569";
      const vsColor = isHeader ? "#1e3a8a" : "#64748b";

      return `
      ${itemBg}
      <text x="${sx + 20}" y="${iy + 4}" font-size="12" font-weight="${isHeader ? "bold" : "600"}" fill="${isHeader ? "#1d4ed8" : "#0f172a"}">${xmlEscape(it.name)}</text>
      <text x="${sx + 165}" y="${iy + 4}" font-size="12" font-weight="${isHeader ? "bold" : "500"}" fill="${decColor}">${xmlEscape(it.dec)}</text>
      <text x="${sx + 265}" y="${iy + 4}" font-size="12" font-weight="${isHeader ? "bold" : "500"}" fill="${encColor}">${xmlEscape(it.enc)}</text>
      <text x="${sx + 360}" y="${iy + 4}" font-size="12" font-weight="${isHeader ? "bold" : "600"}" fill="${ratioColor}">${xmlEscape(it.ratio)}</text>
      <text x="${sx + scW - 20}" y="${iy + 4}" font-size="11.5" font-weight="${isHeader ? "bold" : "500"}" fill="${vsColor}" text-anchor="end">${xmlEscape(it.vs)}</text>
      `;
    }).join("");

    return `
    <g class="scenario-card">
      <rect x="${sx}" y="${sy}" width="${scW}" height="${scH}" rx="10" fill="#ffffff" stroke="#cbd5e1" stroke-width="1.2"/>
      
      <!-- Card Header Strip with bottom divider -->
      <path d="M ${sx} ${sy + 10} Q ${sx} ${sy} ${sx + 10} ${sy} L ${sx + scW - 10} ${sy} Q ${sx + scW} ${sy} ${sx + scW} ${sy + 10} L ${sx + scW} ${sy + 48} L ${sx} ${sy + 48} Z" fill="#f8fafc"/>
      <text x="${sx + 18}" y="${sy + 22}" font-size="13" font-weight="bold" fill="#0f172a">${sc.title}</text>
      <text x="${sx + 18}" y="${sy + 37}" font-size="11" fill="#475569">${sc.sub}</text>

      <rect x="${sx + scW - sc.badgeW - 16}" y="${sy + 12}" width="${sc.badgeW}" height="22" rx="4" fill="#f1f5f9"/>
      <text x="${sx + scW - sc.badgeW / 2 - 16}" y="${sy + 27}" font-size="10.5" font-weight="bold" fill="#1e293b" text-anchor="middle">${xmlEscape(sc.badge)}</text>
      <line x1="${sx}" y1="${sy + 48}" x2="${sx + scW}" y2="${sy + 48}" stroke="#cbd5e1" stroke-width="1"/>

      <!-- Subtable Header with generous top & bottom margins (算法名称行) -->
      <rect x="${sx + 10}" y="${subHeaderY}" width="${scW - 20}" height="${subHeaderH}" rx="5" fill="#f1f5f9"/>
      <text x="${sx + 20}" y="${subHeaderY + 17}" font-size="10.5" font-weight="bold" fill="#334155">${i18n.sc_col_algo}</text>
      <text x="${sx + 165}" y="${subHeaderY + 17}" font-size="10.5" font-weight="bold" fill="#334155">${i18n.sc_col_dec}</text>
      <text x="${sx + 265}" y="${subHeaderY + 17}" font-size="10.5" font-weight="bold" fill="#334155">${i18n.sc_col_enc}</text>
      <text x="${sx + 360}" y="${subHeaderY + 17}" font-size="10.5" font-weight="bold" fill="#334155">${i18n.sc_col_ratio}</text>
      <text x="${sx + scW - 20}" y="${subHeaderY + 17}" font-size="10.5" font-weight="bold" fill="#334155" text-anchor="end">${i18n.sc_col_speedup}</text>

      ${rows}
    </g>
    `;
  }).join("\n");

  const repoUrl = "github.com/webc-site/wedb_embed/tree/main/fastalp";

  return `<?xml version="1.0" encoding="UTF-8"?>
<svg width="${width}" height="${totalH}" viewBox="0 0 ${width} ${totalH}" fill="none" xmlns="http://www.w3.org/2000/svg">
  <!-- Clean White Background -->
  <rect width="100%" height="100%" fill="#ffffff"/>

  <style>
    text { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif; }
  </style>

  <!-- Left: Title + Open-Source Link + Subtitle -->
  <text x="36" y="${topPad + 22}" font-size="22" font-weight="800" fill="#0f172a" letter-spacing="-0.3">${i18n.title}</text>
  <a href="https://${repoUrl}" target="_blank">
    <text x="36" y="${topPad + 44}" font-size="12" font-weight="600" fill="#2563eb">${repoUrl}</text>
  </a>
  <text x="36" y="${topPad + 66}" font-size="12.5" font-weight="500" fill="#334155">${i18n.subtitle}</text>

  <!-- Right: Environment Badge (Exactly aligned in height: 78px) -->
  <g transform="translate(${width - 520}, ${topPad})">
    <rect width="484" height="${headerBoxH}" rx="8" fill="#f8fafc" stroke="#cbd5e1" stroke-width="1"/>
    <text x="16" y="22" font-size="11" font-weight="bold" fill="#0f172a">${i18n.env_badge_title}</text>
    <text x="16" y="44" font-size="11.5" font-weight="500" fill="#1e293b">${xmlEscape(sysEnv.cpu)}</text>
    <text x="16" y="64" font-size="11" fill="#475569">${xmlEscape(sysEnv.toolchain)}</text>
  </g>

  <!-- Section 1: Main Table Container (8 Codecs Overview) -->
  <g transform="translate(0, 0)">
    <text x="36" y="${sec1Y - 14}" font-size="16" font-weight="bold" fill="#0f172a">${i18n.sec1_title}</text>
    <text x="36" y="${sec1Y + 6}" font-size="12" fill="#475569">${i18n.sec1_sub}</text>

    <!-- Legend: Orange-Red for Decode, Sapphire Blue for Encode -->
    <g transform="translate(${width - 320}, ${sec1Y - 12})">
      <rect width="12" height="12" rx="3" fill="#ea580c"/>
      <text x="18" y="10" font-size="11" font-weight="600" fill="#1e293b">${i18n.legend_dec_speed}</text>
      <rect x="135" y="0" width="12" height="12" rx="3" fill="#2563eb"/>
      <text x="153" y="10" font-size="11" font-weight="600" fill="#1e293b">${i18n.legend_enc_speed}</text>
    </g>

    <!-- Table Header Strip (Separated on its own line with 16px bottom margin) -->
    <rect x="32" y="${sec1HeaderY}" width="${width - 64}" height="${sec1HeaderH}" rx="6" fill="#f1f5f9" stroke="#cbd5e1" stroke-width="1"/>
    <text x="48" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_algo}</text>
    <text x="280" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_dec}</text>
    <text x="480" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_dec_vs}</text>
    <text x="635" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_enc}</text>
    <text x="830" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_enc_vs}</text>
    <text x="985" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_ratio}</text>

    <!-- Data Rows -->
    ${sec1RowsSvg}
  </g>

  <!-- Section 2: Industrial Scenarios Comparisons (3x2 Grid) -->
  <g transform="translate(0, 0)">
    <text x="36" y="${sec2Y + 14}" font-size="16" font-weight="bold" fill="#0f172a">${i18n.sec2_title}</text>
    <text x="36" y="${sec2Y + 32}" font-size="12" fill="#475569">${i18n.sec2_sub}</text>

    ${sec2CardsSvg}
  </g>

  <!-- Footer Notes (Multi-line Centered) -->
  <g class="footer-notes">
    ${footerLines.map((line, idx) => `
    <text x="${width / 2}" y="${footerY + idx * footerLineH}" font-size="11.5" font-weight="500" fill="#64748b" text-anchor="middle">${xmlEscape(line)}</text>
    `).join("")}
  </g>
</svg>
`;
};

export default renderSvg;
