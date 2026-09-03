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

  // Mobile-first layout: 720px width, tall vertical infographic
  const width = 720;
  const margin = 24;
  const contentW = width - 2 * margin; // 672px

  const topPad = 32;

  // Header geometry: unified spacing around Environment Card
  const headerBoxH = 66;
  const envCardY = topPad + 90; // Balanced gap from subtitle

  // Section 1: Main Table (8 Codecs Overview)
  const sec1Y = envCardY + headerBoxH + 26;
  const sec1HeaderY = sec1Y + 56;
  const sec1HeaderH = 36;
  const sec1FirstRowY = sec1HeaderY + sec1HeaderH + 10;
  const sec1RowH = 58;
  const sec1TableBottom = sec1FirstRowY + algorithms.length * sec1RowH;

  // Section 2: 6 Industrial Scenario Cards (Single Column 1x6 Stack)
  const sec2Y = sec1TableBottom + 34;
  const scW = contentW; // 672px
  const scH = 224;
  const cardGap = 18;
  const sec2CardsY = sec2Y + 56;

  const sec2Bottom = sec2CardsY + 6 * scH + 5 * cardGap;

  const footerLines = (i18n.footer_lines || [i18n.footer_left]).map((line) =>
    line.replace("{cpu}", sysEnv.cpuModel || "Apple Silicon")
  );
  const footerLineH = 22;
  const footerY = sec2Bottom + 30;
  const totalH = footerY + (footerLines.length - 1) * footerLineH + 38;

  const cppAlgo = algorithms.find((a) => a.algorithm === "cpp_alp") || algorithms[1];
  const cppDec = cppAlgo.paper_31.geomean_dec_gb_s || cppAlgo.paper_31.avg_dec_gb_s || 20.2;
  const cppEnc = cppAlgo.paper_31.geomean_enc_gb_s || cppAlgo.paper_31.avg_enc_gb_s || 0.8;

  // 1. Render Section 1 rows (8 Codecs) - using Geometric Mean
  const sec1RowsSvg = algorithms.map((algo, idx) => {
    const y = sec1FirstRowY + idx * sec1RowH;
    const isFastalp = algo.algorithm === "fastalp";
    const isCpp = algo.algorithm === "cpp_alp";

    const decSpeed = algo.paper_31.geomean_dec_gb_s || algo.paper_31.avg_dec_gb_s;
    const encSpeed = algo.paper_31.geomean_enc_gb_s || algo.paper_31.avg_enc_gb_s;
    const ratio = algo.paper_31.geomean_ratio || algo.paper_31.ratio;

    const rowBg = isFastalp
      ? `<rect x="${margin}" y="${y}" width="${contentW}" height="${sec1RowH - 5}" rx="8" fill="#f0f7ff" stroke="#bfdbfe" stroke-width="1.2"/>`
      : idx % 2 === 1
      ? `<rect x="${margin}" y="${y}" width="${contentW}" height="${sec1RowH - 5}" rx="8" fill="#f8fafc"/>`
      : "";

    const algoDisplayName = isCpp ? "C++ ALP" : algo.display_name;
    const badgeW = isZh ? 42 : 50;
    const highlightBadge = isFastalp
      ? `<rect x="162" y="${y + 8}" width="${badgeW}" height="18" rx="3.5" fill="#2563eb"/><text x="${162 + badgeW / 2}" y="${y + 21.5}" font-size="10" font-weight="bold" fill="#ffffff" text-anchor="middle">${i18n.badge_leader}</text>`
      : isCpp
      ? `<rect x="120" y="${y + 8}" width="${badgeW}" height="18" rx="3.5" fill="#e2e8f0"/><text x="${120 + badgeW / 2}" y="${y + 21.5}" font-size="10" font-weight="600" fill="#475569" text-anchor="middle">${i18n.baseline_text}</text>`
      : "";

    const typeLabel = algo.category === "specialized_float" ? i18n.type_specialized : i18n.type_general;
    const typeColor = algo.category === "specialized_float" ? "#0369a1" : "#64748b";

    let decVs = "";
    if (isFastalp) {
      if (decSpeed >= cppDec) {
        const mult = (decSpeed / cppDec).toFixed(1);
        decVs = isZh ? `较 C++ 快 ${mult}x` : `${mult}x vs C++`;
      } else {
        const pct = ((decSpeed / cppDec) * 100).toFixed(0);
        decVs = isZh ? `达 C++ ${pct}%` : `${pct}% vs C++`;
      }
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
      if (encSpeed >= cppEnc) {
        const mult = (encSpeed / cppEnc).toFixed(1);
        encVs = isZh ? `较 C++ 快 ${mult}x` : `${mult}x vs C++`;
      } else {
        const pct = ((encSpeed / cppEnc) * 100).toFixed(0);
        encVs = isZh ? `达 C++ ${pct}%` : `${pct}% vs C++`;
      }
    } else if (isCpp) {
      encVs = i18n.baseline_text;
    } else {
      const speedup = (encSpeed / cppEnc).toFixed(1) + "x";
      encVs = speedup;
    }

    return `
    <g class="algo-row">
      ${rowBg}
      <!-- Col 1: Algorithm Name & Type -->
      <text x="${margin + 14}" y="${y + 23}" font-size="15" font-weight="${isFastalp ? "800" : "600"}" fill="${isFastalp ? "#1d4ed8" : "#0f172a"}">${xmlEscape(algoDisplayName)}</text>
      ${highlightBadge}
      <text x="${margin + 14}" y="${y + 42}" font-size="11.5" font-weight="500" fill="${typeColor}">${typeLabel}</text>

      <!-- Col 2: Decompress Speed & Comparison -->
      <text x="245" y="${y + 23}" font-size="14.5" font-weight="bold" fill="${isFastalp ? "#c2410c" : "#1e293b"}">${decSpeed.toFixed(1)} GB/s</text>
      <text x="245" y="${y + 42}" font-size="12" font-weight="${isFastalp ? "bold" : "500"}" fill="${isFastalp ? "#ea580c" : "#64748b"}">${decVs}</text>

      <!-- Col 3: Compress Speed & Comparison -->
      <text x="420" y="${y + 23}" font-size="14.5" font-weight="bold" fill="${isFastalp ? "#1d4ed8" : "#1e293b"}">${encSpeed.toFixed(1)} GB/s</text>
      <text x="420" y="${y + 42}" font-size="12" font-weight="${isFastalp ? "bold" : "500"}" fill="${isFastalp ? "#2563eb" : "#64748b"}">${encVs}</text>

      <!-- Col 4: Compression Ratio -->
      <text x="${margin + contentW - 14}" y="${y + 23}" font-size="16" font-weight="bold" fill="${isFastalp ? "#1d4ed8" : "#0f172a"}" text-anchor="end">${ratio.toFixed(2)}x</text>
      <text x="${margin + contentW - 14}" y="${y + 42}" font-size="11" font-weight="500" fill="#94a3b8" text-anchor="end">${isZh ? "几何均值" : "GeoMean"}</text>
    </g>
    `;
  }).join("\n");

  // 2. Section 2: 6 Scenarios Microbenchmarks
  const buildScenarioItems = (sceneKey) => {
    const fourthAlgoMap = {
      scene_sensor: { id: "lz4", name: "LZ4" },
      scene_finance: { id: "snappy", name: "Snappy (snap)" },
      scene_geo: { id: "snappy", name: "Snappy (snap)" },
      scene_health: { id: "zstd", name: "Zstd (Level 3)" },
      scene_macro: { id: "zstd", name: "Zstd (Level 3)" },
      scene_waveform: { id: "zstd", name: "Zstd (Level 3)" },
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
        rawDec: sc.dec_gb_s || 0,
        rawEnc: sc.enc_gb_s || 0,
        rawRatio: sc.ratio || 0,
        dec,
        enc,
        ratio,
        isFastalp: !!item.isFastalp,
      };
    });
  };

  const scenariosData = [
    {
      title: i18n.scene_sensor_title,
      sub: i18n.scene_sensor_sub,
      badge: i18n.scene_sensor_badge,
      badgeW: isZh ? 128 : 138,
      items: buildScenarioItems("scene_sensor")
    },
    {
      title: i18n.scene_finance_title,
      sub: i18n.scene_finance_sub,
      badge: i18n.scene_finance_badge,
      badgeW: isZh ? 120 : 130,
      items: buildScenarioItems("scene_finance")
    },
    {
      title: i18n.scene_geo_title,
      sub: i18n.scene_geo_sub,
      badge: i18n.scene_geo_badge,
      badgeW: isZh ? 120 : 130,
      items: buildScenarioItems("scene_geo")
    },
    {
      title: i18n.scene_health_title,
      sub: i18n.scene_health_sub,
      badge: i18n.scene_health_badge,
      badgeW: isZh ? 120 : 130,
      items: buildScenarioItems("scene_health")
    },
    {
      title: i18n.scene_macro_title,
      sub: i18n.scene_macro_sub,
      badge: i18n.scene_macro_badge,
      badgeW: isZh ? 120 : 130,
      items: buildScenarioItems("scene_macro")
    },
    {
      title: i18n.scene_waveform_title,
      sub: i18n.scene_waveform_sub,
      badge: i18n.scene_waveform_badge,
      badgeW: isZh ? 120 : 130,
      items: buildScenarioItems("scene_waveform")
    }
  ];

  const sec2CardsSvg = scenariosData.map((sc, sIdx) => {
    const sx = margin;
    const sy = sec2CardsY + sIdx * (scH + cardGap);

    const subHeaderY = sy + 50 + 8;
    const subHeaderH = 28;
    const rowsStartY = subHeaderY + subHeaderH + 8;

    const maxDec = Math.max(...sc.items.map(it => it.rawDec));
    const maxEnc = Math.max(...sc.items.map(it => it.rawEnc));
    const maxRatio = Math.max(...sc.items.map(it => it.rawRatio));

    const rows = sc.items.map((it, itIdx) => {
      const iy = rowsStartY + 14 + itIdx * 29;
      const isHeader = it.isFastalp;
      const itemBg = isHeader
        ? `<rect x="${sx + 10}" y="${iy - 14}" width="${scW - 20}" height="29" rx="5.5" fill="#f0f7ff" stroke="#bfdbfe" stroke-width="1.1"/>`
        : itIdx % 2 === 1
        ? `<rect x="${sx + 10}" y="${iy - 14}" width="${scW - 20}" height="29" rx="5.5" fill="#f8fafc"/>`
        : "";

      const isMaxDec = Math.abs(it.rawDec - maxDec) < 0.05;
      const isMaxEnc = Math.abs(it.rawEnc - maxEnc) < 0.05;
      const isMaxRatio = Math.abs(it.rawRatio - maxRatio) < 0.05;

      const decColor = isMaxDec ? "#ea580c" : "#475569";
      const decWeight = isMaxDec ? "bold" : "500";

      const encColor = isMaxEnc ? "#2563eb" : "#475569";
      const encWeight = isMaxEnc ? "bold" : "500";

      const ratioColor = isMaxRatio ? "#059669" : "#475569";
      const ratioWeight = isMaxRatio ? "bold" : "500";

      return `
      ${itemBg}
      <text x="${sx + 20}" y="${iy + 5}" font-size="13.5" font-weight="${isHeader ? "bold" : "600"}" fill="${isHeader ? "#1d4ed8" : "#0f172a"}">${xmlEscape(it.name)}</text>
      <text x="${sx + 250}" y="${iy + 5}" font-size="13.5" font-weight="${decWeight}" fill="${decColor}" text-anchor="end">${xmlEscape(it.dec)}</text>
      <text x="${sx + 395}" y="${iy + 5}" font-size="13.5" font-weight="${encWeight}" fill="${encColor}" text-anchor="end">${xmlEscape(it.enc)}</text>
      <text x="${sx + scW - 20}" y="${iy + 5}" font-size="13.5" font-weight="${ratioWeight}" fill="${ratioColor}" text-anchor="end">${xmlEscape(it.ratio)}</text>
      `;
    }).join("");

    return `
    <g class="scenario-card">
      <rect x="${sx}" y="${sy}" width="${scW}" height="${scH}" rx="10" fill="#ffffff" stroke="#cbd5e1" stroke-width="1.2"/>
      
      <!-- Card Header Strip with bottom divider -->
      <path d="M ${sx} ${sy + 10} Q ${sx} ${sy} ${sx + 10} ${sy} L ${sx + scW - 10} ${sy} Q ${sx + scW} ${sy} ${sx + scW} ${sy + 10} L ${sx + scW} ${sy + 50} L ${sx} ${sy + 50} Z" fill="#f8fafc"/>
      <text x="${sx + 16}" y="${sy + 23}" font-size="16" font-weight="bold" fill="#0f172a">${sc.title}</text>
      <text x="${sx + 16}" y="${sy + 41}" font-size="12.5" fill="#475569">${sc.sub}</text>

      <rect x="${sx + scW - sc.badgeW - 14}" y="${sy + 13}" width="${sc.badgeW}" height="24" rx="4" fill="#f1f5f9"/>
      <text x="${sx + scW - sc.badgeW / 2 - 14}" y="${sy + 29}" font-size="12" font-weight="bold" fill="#1e293b" text-anchor="middle">${xmlEscape(sc.badge)}</text>
      <line x1="${sx}" y1="${sy + 50}" x2="${sx + scW}" y2="${sy + 50}" stroke="#cbd5e1" stroke-width="1"/>

      <!-- Subtable Header -->
      <rect x="${sx + 10}" y="${subHeaderY}" width="${scW - 20}" height="${subHeaderH}" rx="5" fill="#f1f5f9"/>
      <text x="${sx + 20}" y="${subHeaderY + 18}" font-size="12" font-weight="bold" fill="#334155">${i18n.sc_col_algo}</text>
      <text x="${sx + 250}" y="${subHeaderY + 18}" font-size="12" font-weight="bold" fill="#334155" text-anchor="end">${i18n.sc_col_dec}</text>
      <text x="${sx + 395}" y="${subHeaderY + 18}" font-size="12" font-weight="bold" fill="#334155" text-anchor="end">${i18n.sc_col_enc}</text>
      <text x="${sx + scW - 20}" y="${subHeaderY + 18}" font-size="12" font-weight="bold" fill="#334155" text-anchor="end">${i18n.sc_col_ratio}</text>

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

  <!-- Top Title Block -->
  <text x="${margin}" y="${topPad + 24}" font-size="24" font-weight="800" fill="#0f172a" letter-spacing="-0.3">${i18n.title}</text>
  <a href="https://${repoUrl}" target="_blank">
    <text x="${margin}" y="${topPad + 49}" font-size="13.5" font-weight="600" fill="#2563eb">${repoUrl}</text>
  </a>
  <text x="${margin}" y="${topPad + 72}" font-size="13" font-weight="500" fill="#475569">${i18n.subtitle}</text>

  <!-- Environment Card (Unified 20px top margin, 26px bottom margin, centered text) -->
  <g transform="translate(${margin}, ${envCardY})">
    <rect width="${contentW}" height="${headerBoxH}" rx="8" fill="#f8fafc" stroke="#cbd5e1" stroke-width="1"/>
    <text x="16" y="28" font-size="13" font-weight="bold" fill="#0f172a">${i18n.env_badge_title} · ${xmlEscape(sysEnv.cpu)}</text>
    <text x="16" y="51" font-size="12" fill="#475569">${xmlEscape(sysEnv.toolchain)}</text>
  </g>

  <!-- Section 1: Main Table Container (8 Codecs Overview, No Legend) -->
  <g transform="translate(0, 0)">
    <text x="${margin}" y="${sec1Y + 20}" font-size="18" font-weight="bold" fill="#0f172a">${i18n.sec1_title}</text>
    <text x="${margin}" y="${sec1Y + 42}" font-size="13" fill="#475569">${i18n.sec1_sub}</text>

    <!-- Table Header Strip -->
    <rect x="${margin}" y="${sec1HeaderY}" width="${contentW}" height="${sec1HeaderH}" rx="6" fill="#f1f5f9" stroke="#cbd5e1" stroke-width="1"/>
    <text x="${margin + 14}" y="${sec1HeaderY + 23}" font-size="13" font-weight="bold" fill="#1e293b">${i18n.col_algo}</text>
    <text x="245" y="${sec1HeaderY + 23}" font-size="13" font-weight="bold" fill="#1e293b">${i18n.col_dec}</text>
    <text x="420" y="${sec1HeaderY + 23}" font-size="13" font-weight="bold" fill="#1e293b">${i18n.col_enc}</text>
    <text x="${margin + contentW - 14}" y="${sec1HeaderY + 23}" font-size="13" font-weight="bold" fill="#1e293b" text-anchor="end">${i18n.col_ratio}</text>

    <!-- Data Rows -->
    ${sec1RowsSvg}
  </g>

  <!-- Section 2: Industrial Scenarios Comparisons (Single Column Vertical Stack) -->
  <g transform="translate(0, 0)">
    <text x="${margin}" y="${sec2Y + 20}" font-size="18" font-weight="bold" fill="#0f172a">${i18n.sec2_title}</text>
    <text x="${margin}" y="${sec2Y + 42}" font-size="13" fill="#475569">${i18n.sec2_sub}</text>

    ${sec2CardsSvg}
  </g>

  <!-- Footer Notes (Multi-line Centered) -->
  <g class="footer-notes">
    ${footerLines.map((line, idx) => `
    <text x="${width / 2}" y="${footerY + idx * footerLineH}" font-size="12" font-weight="500" fill="#64748b" text-anchor="middle">${xmlEscape(line)}</text>
    `).join("")}
  </g>
</svg>
`;
};

export default renderSvg;
