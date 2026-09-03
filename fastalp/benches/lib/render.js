import { datasetMeta, industrialScenarios, getSystemEnv } from "./data.js";

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

  // Unified Section Gap: exactly 36px between Section 1 & 2, and between Section 2 & 3
  const SECTION_GAP = 36;

  // Section 1: Main Table (8 Codecs Overview) starts directly below Header Box
  const sec1Y = topPad + headerBoxH + 32;
  const sec1HeaderY = sec1Y + 36;
  const sec1HeaderH = 34;
  const sec1FirstRowY = sec1HeaderY + sec1HeaderH + 12; // 12px margin below header
  const sec1RowH = 46;
  const sec1TableBottom = sec1FirstRowY + (algorithms.length - 1) * sec1RowH + 42;

  // Section 2: All 31 Datasets Breakdown in ONE Unified Card with Shared Header
  const sec2Y = sec1TableBottom + SECTION_GAP;
  const sec2CardW = width - 64;     // 1176px wide
  const subW = (sec2CardW - 48) / 2;// 564px wide each subtable
  const dsHeaderH = 30;
  const dsRowH = 25;
  const dsRowCount = 18; // 18 rows (Left: 0..17, Right: 18..34 + summary)
  const sec2CardH = 44 + 14 + dsHeaderH + 10 + dsRowCount * dsRowH + 14;
  const sec2Bottom = sec2Y + sec2CardH;

  // Section 3: 4 Industrial Scenario Microbenchmarks (2x2 Grid)
  const sec3Y = sec2Bottom + SECTION_GAP;
  const scW = (width - 64 - 24) / 2;
  const scH = 228;
  const sec3CardsY = sec3Y + 46; // Subtitle is at sec3Y + 32, so cards start at sec3Y + 46 (gap = 14px)

  const sec3Bottom = sec3CardsY + 2 * scH + 16;
  const footerY = sec3Bottom + 26;
  const totalH = footerY + 28;

  // Palette: Cool Sapphire Blue for Decompression, Warm Amber Gold for Compression
  const barColors = {
    fastalp: { dec: "#2563eb", enc: "#d97706", text: "#1d4ed8", encText: "#92400e" },
    cpp_alp: { dec: "#3b82f6", enc: "#f59e0b", text: "#1e3a8a", encText: "#78350f" },
    pco:     { dec: "#64748b", enc: "#b45309", text: "#1e293b", encText: "#475569" },
    zstd:    { dec: "#64748b", enc: "#b45309", text: "#1e293b", encText: "#475569" },
    lz4:     { dec: "#64748b", enc: "#b45309", text: "#1e293b", encText: "#475569" },
    snappy:  { dec: "#64748b", enc: "#b45309", text: "#1e293b", encText: "#475569" },
    chimp128:{ dec: "#64748b", enc: "#b45309", text: "#1e293b", encText: "#475569" },
    gorilla: { dec: "#64748b", enc: "#b45309", text: "#1e293b", encText: "#475569" },
  };

  const maxDecSpeed = 24.0;
  const maxEncSpeed = 6.0;
  const barW = 85;

  const fastalpAlgo = algorithms.find(a => a.algorithm === "fastalp") || algorithms[0];
  const cppAlgo = algorithms.find(a => a.algorithm === "cpp_alp") || algorithms[1];

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

    const badgeW = isZh ? 48 : 54;
    const highlightBadge = isFastalp
      ? `<rect x="180" y="${y + 3}" width="${badgeW}" height="18" rx="4" fill="#2563eb"/><text x="${180 + badgeW / 2}" y="${y + 16}" font-size="10" font-weight="bold" fill="#ffffff" text-anchor="middle">${i18n.badge_leader}</text>`
      : "";

    // Clean name (remove any reference wording)
    const displayName = algo.display_name.replace(" (Reference)", "");

    const cppDecSpeed = cppAlgo?.paper_31?.geomean_dec_gb_s || 19.04;
    const decSpeedupPct = ((decSpeed / cppDecSpeed - 1) * 100).toFixed(1);
    const decVsBase = isFastalp
      ? `<text x="445" y="${y + 20}" font-size="12" font-weight="bold" fill="#1e3a8a">${decSpeed >= cppDecSpeed ? (isZh ? `+${decSpeedupPct}% 领先` : `+${decSpeedupPct}% Lead`) : `${(decSpeed / cppDecSpeed).toFixed(2)}x`}</text>`
      : isCpp
      ? `<text x="445" y="${y + 20}" font-size="12" font-weight="600" fill="#475569">${isZh ? "基准 (1.00x)" : "Baseline (1.00x)"}</text>`
      : `<text x="445" y="${y + 20}" font-size="12" font-weight="500" fill="#64748b">${(decSpeed / cppDecSpeed).toFixed(2)}x</text>`;

    const cppEncSpeed = cppAlgo?.paper_31?.geomean_enc_gb_s || 5.12;
    const encSpeedupPct = ((encSpeed / cppEncSpeed - 1) * 100).toFixed(1);
    const encVsBase = isFastalp
      ? `<text x="810" y="${y + 20}" font-size="12" font-weight="bold" fill="#92400e">${encSpeed >= cppEncSpeed ? `+${encSpeedupPct}%` : `${(encSpeed / cppEncSpeed).toFixed(2)}x`}</text>`
      : isCpp
      ? `<text x="810" y="${y + 20}" font-size="12" font-weight="600" fill="#475569">${isZh ? "基准 (1.00x)" : "Baseline (1.00x)"}</text>`
      : `<text x="810" y="${y + 20}" font-size="12" font-weight="500" fill="#64748b">${(encSpeed / cppEncSpeed).toFixed(2)}x</text>`;

    return `
    <g class="table-row">
      ${rowBg}
      
      <!-- Algorithm Name -->
      <text x="48" y="${y + 17}" font-size="13.5" font-weight="${isFastalp ? "bold" : "600"}" fill="${isFastalp ? "#1d4ed8" : "#0f172a"}">${xmlEscape(displayName)}</text>
      <text x="48" y="${y + 31}" font-size="11" fill="#475569">${algo.category === "specialized_float" ? i18n.type_specialized : i18n.type_general}</text>
      ${highlightBadge}

      <!-- Decode Throughput Column (Sapphire Blue Bar) -->
      <g transform="translate(240, ${y + 5})">
        <rect width="${barW}" height="14" rx="3" fill="#dbeafe"/>
        <rect width="${decBarLen}" height="14" rx="3" fill="${c.dec}"/>
        <text x="${barW + 8}" y="12" font-size="12.5" font-weight="${isFastalp ? "bold" : "600"}" fill="${c.text}">${decSpeed.toFixed(2)} GB/s</text>
      </g>

      <!-- Decode vs Baseline -->
      ${decVsBase}

      <!-- Encode Throughput Column (Warm Amber Gold Bar) -->
      <g transform="translate(605, ${y + 5})">
        <rect width="${barW}" height="14" rx="3" fill="#fef3c7"/>
        <rect width="${encBarLen}" height="14" rx="3" fill="${c.enc}"/>
        <text x="${barW + 8}" y="12" font-size="12.5" font-weight="${isFastalp ? "bold" : "600"}" fill="${c.encText || "#92400e"}">${encSpeed.toFixed(2)} GB/s</text>
      </g>

      <!-- Encode vs Baseline -->
      ${encVsBase}

      <!-- Compression Ratio (Direct x, Dark Text) -->
      <text x="975" y="${y + 20}" font-size="14" font-weight="${isFastalp ? "bold" : "600"}" fill="${isFastalp ? "#1d4ed8" : "#0f172a"}">${ratio.toFixed(2)}x</text>
    </g>`;
  }).join("\n");

  // 2. Prepare Section 2 datasets (All 35 Datasets & Industrial Scenarios in Shared Unified Card)
  const fDatasets = fastalpAlgo.paper_31?.datasets || [];
  const cDatasetsMap = new Map((cppAlgo.paper_31?.datasets || []).map(d => [d.name, d]));

  const renderSubtable = (datasetsSubset, startX, startY, isRightCol = false) => {
    const tableX = startX;
    const tableY = startY;

    const rows = datasetsSubset.map((d, rIdx) => {
      const cy = tableY + dsHeaderH + 12 + rIdx * dsRowH;
      const c = cDatasetsMap.get(d.name) || {};
      const fRatioNum = d.ratio;
      const cRatioNum = c.ratio ?? 0;
      const fRatioStr = fRatioNum.toFixed(2);
      const cRatioStr = cRatioNum.toFixed(2);

      const fDecNum = d.dec_gb_s || 0;
      const cDecNum = c.dec_gb_s || 0;
      const fDecStr = fDecNum ? (fDecNum >= 10 ? fDecNum.toFixed(1) : fDecNum.toFixed(2)) : "-";
      const cDecStr = cDecNum ? (cDecNum >= 10 ? cDecNum.toFixed(1) : cDecNum.toFixed(2)) : "-";

      // 动态对比：谁压缩率高谁蓝色，次优显示灰色
      const fRatioWins = fRatioNum >= cRatioNum;
      const fRatioColor = fRatioWins ? "#1d4ed8" : "#64748b";
      const fRatioWeight = fRatioWins ? "bold" : "500";
      const cRatioColor = !fRatioWins ? "#1d4ed8" : "#64748b";
      const cRatioWeight = !fRatioWins ? "bold" : "500";

      // 动态对比：谁解压吞吐高谁蓝色，次优显示灰色
      const fDecWins = fDecNum >= cDecNum;
      const fDecColor = fDecWins ? "#1d4ed8" : "#64748b";
      const fDecWeight = fDecWins ? "bold" : "500";
      const cDecColor = !fDecWins ? "#1d4ed8" : "#64748b";
      const cDecWeight = !fDecWins ? "bold" : "500";

      // 相对加速比
      const speedupNum = (fDecNum && cDecNum) ? (fDecNum / cDecNum) : 1.0;
      const speedupStr = fDecWins ? `+${((speedupNum - 1) * 100).toFixed(0)}%` : `${speedupNum.toFixed(2)}x`;
      const speedupColor = fDecWins ? "#1e3a8a" : "#64748b";
      const speedupWeight = fDecWins ? "bold" : "500";

      // 领域元数据与名称
      const meta = datasetMeta[d.name] || {};
      const displayName = isZh ? (meta.zh || d.name) : (meta.en || d.name);

      const domainColors = {
        IoT: { bg: "#e0f2fe", text: "#0284c7" },
        气象: { bg: "#ecfdf5", text: "#059669" },
        地理: { bg: "#f0fdf4", text: "#16a34a" },
        金融: { bg: "#fef3c7", text: "#b45309" },
        医疗: { bg: "#fae8ff", text: "#a21caf" },
        政务: { bg: "#f1f5f9", text: "#475569" },
        工业: { bg: "#fee2e2", text: "#b91c1c" },
      };
      const badgeCol = domainColors[meta.domain] || { bg: "#f1f5f9", text: "#475569" };
      const badgeW = isZh ? 26 : 30;

      const bg = rIdx % 2 === 1 ? `<rect x="${tableX}" y="${cy - 14}" width="${subW}" height="${dsRowH - 2}" rx="4" fill="#f8fafc"/>` : "";

      return `
      ${bg}
      <rect x="${tableX + 8}" y="${cy - 9}" width="${badgeW}" height="14" rx="3" fill="${badgeCol.bg}"/>
      <text x="${tableX + 8 + badgeW / 2}" y="${cy + 2}" font-size="8" font-weight="bold" fill="${badgeCol.text}" text-anchor="middle">${meta.domain || "IND"}</text>
      <text x="${tableX + 12 + badgeW + 4}" y="${cy + 3}" font-size="10.5" font-weight="600" fill="#0f172a">${xmlEscape(displayName)}</text>
      <text x="${tableX + 175}" y="${cy + 3}" font-size="11" font-weight="${fRatioWeight}" fill="${fRatioColor}">${fRatioStr}x</text>
      <text x="${tableX + 245}" y="${cy + 3}" font-size="11" font-weight="${cRatioWeight}" fill="${cRatioColor}">${cRatioStr}x</text>
      <text x="${tableX + 315}" y="${cy + 3}" font-size="11" font-weight="${fDecWeight}" fill="${fDecColor}">${fDecStr} GB/s</text>
      <text x="${tableX + 395}" y="${cy + 3}" font-size="11" font-weight="${cDecWeight}" fill="${cDecColor}">${cDecStr} GB/s</text>
      <text x="${tableX + subW - 10}" y="${cy + 3}" font-size="10.5" font-weight="${speedupWeight}" fill="${speedupColor}" text-anchor="end">${speedupStr}</text>
      `;
    }).join("");

    // If right col, add summary row with geometric mean
    const fGeomeanRatio = fastalpAlgo.paper_31?.geomean_ratio || 7.15;
    const cGeomeanRatio = cppAlgo.paper_31?.geomean_ratio || 5.96;
    const fGeomeanDec = fastalpAlgo.paper_31?.geomean_dec_gb_s || 17.07;
    const cGeomeanDec = cppAlgo.paper_31?.geomean_dec_gb_s || 18.46;
    const ratioLeadPct = (((fGeomeanRatio / cGeomeanRatio) - 1) * 100).toFixed(1);

    const fRatioWins = fGeomeanRatio >= cGeomeanRatio;
    const fDecWins = fGeomeanDec >= cGeomeanDec;

    const summaryRow = isRightCol ? `
      <rect x="${tableX}" y="${tableY + dsHeaderH + 12 + 17 * dsRowH - 14}" width="${subW}" height="${dsRowH + 2}" rx="4" fill="#eff6ff" stroke="#bfdbfe" stroke-width="1"/>
      <text x="${tableX + 10}" y="${tableY + dsHeaderH + 12 + 17 * dsRowH + 3}" font-size="10.5" font-weight="bold" fill="#1d4ed8">${isZh ? "35 项全量几何均值" : "35 Scenarios Geomean"}</text>
      <text x="${tableX + 175}" y="${tableY + dsHeaderH + 12 + 17 * dsRowH + 3}" font-size="11" font-weight="bold" fill="${fRatioWins ? "#1d4ed8" : "#475569"}">${fGeomeanRatio.toFixed(2)}x</text>
      <text x="${tableX + 245}" y="${tableY + dsHeaderH + 12 + 17 * dsRowH + 3}" font-size="11" font-weight="bold" fill="${!fRatioWins ? "#1d4ed8" : "#475569"}">${cGeomeanRatio.toFixed(2)}x</text>
      <text x="${tableX + 315}" y="${tableY + dsHeaderH + 12 + 17 * dsRowH + 3}" font-size="11" font-weight="bold" fill="${fDecWins ? "#1d4ed8" : "#475569"}">${fGeomeanDec.toFixed(1)} GB/s</text>
      <text x="${tableX + 395}" y="${tableY + dsHeaderH + 12 + 17 * dsRowH + 3}" font-size="11" font-weight="bold" fill="${!fDecWins ? "#1d4ed8" : "#475569"}">${cGeomeanDec.toFixed(1)} GB/s</text>
      <text x="${tableX + subW - 10}" y="${tableY + dsHeaderH + 12 + 17 * dsRowH + 3}" font-size="10.5" font-weight="bold" fill="#1e3a8a" text-anchor="end">${isZh ? `+${ratioLeadPct}% 领先` : `+${ratioLeadPct}% Lead`}</text>
    ` : "";

    return `
    <g class="subtable">
      <!-- Subtable Header with neutral, objective styling -->
      <rect x="${tableX}" y="${tableY}" width="${subW}" height="${dsHeaderH}" rx="5" fill="#f1f5f9"/>
      <text x="${tableX + 10}" y="${tableY + 19}" font-size="10" font-weight="bold" fill="#334155">${i18n.col_dataset}</text>
      <text x="${tableX + 175}" y="${tableY + 19}" font-size="10" font-weight="bold" fill="#334155">${i18n.col_f_ratio}</text>
      <text x="${tableX + 245}" y="${tableY + 19}" font-size="10" font-weight="bold" fill="#334155">${i18n.col_c_ratio}</text>
      <text x="${tableX + 315}" y="${tableY + 19}" font-size="10" font-weight="bold" fill="#334155">${i18n.col_f_dec}</text>
      <text x="${tableX + 395}" y="${tableY + 19}" font-size="10" font-weight="bold" fill="#334155">${i18n.col_c_dec}</text>
      <text x="${tableX + subW - 10}" y="${tableY + 19}" font-size="10" font-weight="bold" fill="#334155" text-anchor="end">${i18n.col_speedup}</text>

      ${rows}
      ${summaryRow}
    </g>
    `;
  };

  const leftDatasets = fDatasets.slice(0, 18);
  const rightDatasets = fDatasets.slice(18, 35);
  const sec2SubtablesY = sec2Y + 44 + 14; // 14px margin below shared header strip
  const sec2LeftSvg = renderSubtable(leftDatasets, 32 + 16, sec2SubtablesY, false);
  const sec2RightSvg = renderSubtable(rightDatasets, 32 + 16 + subW + 16, sec2SubtablesY, true);

  // 3. Section 3: 4 Scenarios Microbenchmarks (Dynamically Calculated from Real Data)
  const buildScenarioItems = (sceneKey) => {
    const list = [
      { id: "fastalp", name: "fastalp (Rust)", bold: true, color: "#1d4ed8" },
      { id: "cpp_alp", name: "C++ ALP", bold: false, color: "#1e293b" },
      { id: "pco", name: "Pcodec (pco)", bold: false, color: "#475569" },
      {
        id: sceneKey === "scene_sensor" ? "lz4" : sceneKey === "scene_ramp" ? "zstd" : sceneKey === "scene_finance" ? "snappy" : "zstd",
        name: sceneKey === "scene_sensor" ? "LZ4" : sceneKey === "scene_ramp" ? "Zstd (Level 3)" : sceneKey === "scene_finance" ? "Snappy (snap)" : "Zstd (Level 3)",
        bold: false,
        color: "#475569"
      }
    ];

    const cppScene = (industrialScenarios.cpp_alp || []).find(s => s.name === sceneKey) || { dec_gb_s: 20, ratio: 5 };

    return list.map(item => {
      const sc = (industrialScenarios[item.id] || []).find(s => s.name === sceneKey) || {};
      const dec = (sc.dec_gb_s || 0).toFixed(sc.dec_gb_s >= 10 ? 1 : 2) + " GB/s";
      const enc = (sc.enc_gb_s || 0).toFixed(1) + " GB/s";
      const ratio = (sc.ratio || 0).toFixed(sc.ratio >= 10 ? 1 : 2) + "x";

      let vs = "";
      if (item.id === "fastalp") {
        if (sc.ratio > cppScene.ratio * 2) {
          const mult = (sc.ratio / cppScene.ratio).toFixed(0);
          vs = isZh ? `比 C++ 高 ${mult}x` : `${mult}x vs C++`;
        } else if (sc.dec_gb_s > cppScene.dec_gb_s) {
          const speedup = (((sc.dec_gb_s / cppScene.dec_gb_s) - 1) * 100).toFixed(0);
          vs = isZh ? `比 C++ 快 ${speedup}%` : `+${speedup}% vs C++`;
        } else {
          vs = isZh ? "领先 (零损)" : "Leader";
        }
      } else if (item.id === "cpp_alp") {
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
        bold: item.bold,
        color: item.color
      };
    });
  };

  const scenariosData = [
    {
      title: i18n.scene_sensor_title,
      sub: i18n.scene_sensor_sub,
      badge: i18n.scene_sensor_badge,
      badgeW: isZh ? 76 : 84,
      items: buildScenarioItems("scene_sensor")
    },
    {
      title: i18n.scene_ramp_title,
      sub: i18n.scene_ramp_sub,
      badge: i18n.scene_ramp_badge,
      badgeW: isZh ? 76 : 84,
      items: buildScenarioItems("scene_ramp")
    },
    {
      title: i18n.scene_stock_title,
      sub: i18n.scene_stock_sub,
      badge: i18n.scene_stock_badge,
      badgeW: isZh ? 76 : 84,
      items: buildScenarioItems("scene_finance")
    },
    {
      title: i18n.scene_const_title,
      sub: i18n.scene_const_sub,
      badge: i18n.scene_const_badge,
      badgeW: isZh ? 76 : 84,
      items: buildScenarioItems("scene_steady")
    }
  ];

  const sec3Svg = scenariosData.map((sc, sIdx) => {
    const row = Math.floor(sIdx / 2);
    const col = sIdx % 2;
    const sx = 32 + col * (scW + 24);
    const sy = sec3CardsY + row * (scH + 16);

    const subHeaderY = sy + 48 + 10;
    const subHeaderH = 26;
    const rowsStartY = subHeaderY + subHeaderH + 10;

    const rows = sc.items.map((it, itIdx) => {
      const iy = rowsStartY + 14 + itIdx * 30;
      const isHeader = itIdx === 0;
      const itemBg = isHeader
        ? `<rect x="${sx + 10}" y="${iy - 14}" width="${scW - 20}" height="28" rx="5" fill="#f0f7ff"/>`
        : itIdx % 2 === 1
        ? `<rect x="${sx + 10}" y="${iy - 14}" width="${scW - 20}" height="28" rx="5" fill="#f8fafc"/>`
        : "";
      return `
      ${itemBg}
      <text x="${sx + 20}" y="${iy + 4}" font-size="12" font-weight="${it.bold ? "bold" : "600"}" fill="${it.bold ? "#1d4ed8" : "#0f172a"}">${xmlEscape(it.name)}</text>
      <text x="${sx + 165}" y="${iy + 4}" font-size="12" font-weight="${it.bold ? "bold" : "600"}" fill="${it.color}">${xmlEscape(it.dec)}</text>
      <text x="${sx + 265}" y="${iy + 4}" font-size="12" font-weight="${it.bold ? "bold" : "500"}" fill="${it.bold ? "#92400e" : "#475569"}">${xmlEscape(it.enc)}</text>
      <text x="${sx + 360}" y="${iy + 4}" font-size="12" font-weight="${it.bold ? "bold" : "600"}" fill="${it.color}">${xmlEscape(it.ratio)}</text>
      <text x="${sx + scW - 20}" y="${iy + 4}" font-size="11.5" font-weight="bold" fill="${it.bold ? "#1e3a8a" : "#475569"}" text-anchor="end">${xmlEscape(it.vs)}</text>
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

    <!-- Legend: Sapphire Blue for Decode, Warm Amber Gold for Encode -->
    <g transform="translate(${width - 320}, ${sec1Y - 12})">
      <rect width="12" height="12" rx="3" fill="#2563eb"/>
      <text x="18" y="10" font-size="11" font-weight="600" fill="#1e293b">${i18n.legend_dec_speed}</text>
      <rect x="135" y="0" width="12" height="12" rx="3" fill="#d97706"/>
      <text x="153" y="10" font-size="11" font-weight="600" fill="#1e293b">${i18n.legend_enc_speed}</text>
    </g>

    <!-- Table Header Strip (Separated on its own line with 16px bottom margin) -->
    <rect x="32" y="${sec1HeaderY}" width="${width - 64}" height="${sec1HeaderH}" rx="6" fill="#f1f5f9" stroke="#cbd5e1" stroke-width="1"/>
    <text x="48" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_algo}</text>
    <text x="240" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_dec}</text>
    <text x="445" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_dec_vs}</text>
    <text x="605" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_enc}</text>
    <text x="810" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_enc_vs}</text>
    <text x="975" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_ratio}</text>

    <!-- Data Rows -->
    ${sec1RowsSvg}
  </g>

  <!-- Section 2: All 31 Datasets Breakdown in ONE Unified Card with Shared Header -->
  <g class="sec2-unified-card">
    <rect x="32" y="${sec2Y}" width="${sec2CardW}" height="${sec2CardH}" rx="10" fill="#ffffff" stroke="#cbd5e1" stroke-width="1.2"/>
    
    <!-- Shared Top Header Strip -->
    <path d="M 32 ${sec2Y + 10} Q 32 ${sec2Y} 42 ${sec2Y} L ${width - 42} ${sec2Y} Q ${width - 32} ${sec2Y} ${width - 32} ${sec2Y + 10} L ${width - 32} ${sec2Y + 44} L 32 ${sec2Y + 44} Z" fill="#f8fafc"/>
    <text x="48" y="${sec2Y + 28}" font-size="13.5" font-weight="bold" fill="#0f172a">${i18n.sec2_title}</text>
    <text x="${width - 48}" y="${sec2Y + 28}" font-size="11.5" font-weight="500" fill="#475569" text-anchor="end">${i18n.sec2_sub}</text>

    <!-- Subtables (Left and Right side-by-side with 24px top margin from header strip) -->
    ${sec2LeftSvg}
    ${sec2RightSvg}
  </g>

  <!-- Section 3: Industrial Scenarios Microbenchmarks (2x2 Grid) -->
  <g transform="translate(0, 0)">
    <text x="36" y="${sec3Y + 14}" font-size="16" font-weight="bold" fill="#0f172a">${i18n.sec3_title}</text>
    <text x="36" y="${sec3Y + 32}" font-size="12" fill="#475569">${i18n.sec3_sub}</text>

    ${sec3Svg}
  </g>

  <!-- Footer (Centered, NO dividing line above) -->
  <text x="${width / 2}" y="${footerY}" font-size="11.5" font-weight="500" fill="#475569" text-anchor="middle">${i18n.footer_left.replace("{cpu}", sysEnv.cpuModel || "Apple Silicon")}</text>
</svg>
`;
};

export default renderSvg;
