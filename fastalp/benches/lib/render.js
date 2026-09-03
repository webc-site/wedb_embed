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

  const i18n = Object.fromEntries(
    Object.entries(rawI18n).map(([k, v]) => [k, typeof v === "string" ? xmlEscape(v) : v])
  );

  const width = 1240;

  // Header geometry: Title block and Environment Box are exactly the same height (78px)
  const topPad = 36;
  const headerBoxH = 78;

  // Section 1: Main Table (8 Codecs Overview) starts directly below Header Box with 34px margin
  const sec1Y = topPad + headerBoxH + 34;
  const sec1HeaderY = sec1Y + 36;
  const sec1HeaderH = 34;
  const sec1FirstRowY = sec1HeaderY + sec1HeaderH + 16; // 16px generous margin below header
  const sec1RowH = 46;
  const sec1H = (sec1FirstRowY - sec1Y) + algorithms.length * sec1RowH;

  // Section 2: All 31 Datasets Breakdown in ONE Unified Card with Shared Header
  const sec2Y = sec1Y + sec1H + 54; // 54px margin above Section 2
  const sec2CardW = width - 64;     // 1176px wide
  const subW = (sec2CardW - 48) / 2;// 564px wide each subtable
  const dsHeaderH = 32;
  const dsRowH = 26;
  const dsRowCount = 16; // 16 rows (Left: 0..15, Right: 16..30 + summary)
  const sec2CardH = 48 + 24 + dsHeaderH + 14 + dsRowCount * dsRowH + 16;
  const sec2H = sec2CardH;

  // Section 3: 4 Industrial Scenario Microbenchmarks (2x2 Grid)
  const sec3Y = sec2Y + sec2H + 54; // 54px margin above Section 3
  const scW = (width - 64 - 24) / 2;
  const scH = 228;
  const sec3H = 44 + (scH * 2) + 20;

  const totalH = sec3Y + sec3H + 52;

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

  const maxDecSpeed = 32.0;
  const maxEncSpeed = 8.0;
  const barW = 140;

  const fastalpAlgo = algorithms.find(a => a.algorithm === "fastalp") || algorithms[0];
  const cppAlgo = algorithms.find(a => a.algorithm === "cpp_alp") || algorithms[1];

  // 1. Render Section 1 rows (8 Codecs)
  const sec1RowsSvg = algorithms.map((algo, idx) => {
    const y = sec1FirstRowY + idx * sec1RowH;
    const isFastalp = algo.algorithm === "fastalp";
    const isCpp = algo.algorithm === "cpp_alp";
    const c = barColors[algo.algorithm] || barColors.gorilla;

    const decSpeed = algo.paper_31.avg_dec_gb_s;
    const encSpeed = algo.paper_31.avg_enc_gb_s;
    const decBarLen = Math.max(6, Math.min(barW, (decSpeed / maxDecSpeed) * barW));
    const encBarLen = Math.max(6, Math.min(barW, (encSpeed / maxEncSpeed) * barW));

    const ratio = algo.paper_31.ratio;
    const totalKB = (algo.paper_31.total_compressed_bytes / 1024).toFixed(1);

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

    const cppDecSpeed = cppAlgo?.paper_31?.avg_dec_gb_s || 20.0;
    const speedupPct = ((decSpeed / cppDecSpeed - 1) * 100).toFixed(1);
    const vsBase = isFastalp
      ? `<text x="1110" y="${y + 20}" font-size="12.5" font-weight="bold" fill="#1e3a8a">${isZh ? `比 C++ 快 ${speedupPct}%` : `+${speedupPct}% vs C++`}</text>`
      : isCpp
      ? `<text x="1110" y="${y + 20}" font-size="12" font-weight="600" fill="#475569">${i18n.baseline_text}</text>`
      : `<text x="1110" y="${y + 20}" font-size="12" font-weight="500" fill="#64748b">${(algo.paper_31.avg_dec_gb_s / algorithms[0].paper_31.avg_dec_gb_s).toFixed(2)}${isZh ? "x 吞吐" : "x speed"}</text>`;

    return `
    <g class="table-row">
      ${rowBg}
      
      <!-- Algorithm Name -->
      <text x="48" y="${y + 17}" font-size="13.5" font-weight="${isFastalp ? "bold" : "600"}" fill="${isFastalp ? "#1d4ed8" : "#0f172a"}">${xmlEscape(displayName)}</text>
      <text x="48" y="${y + 31}" font-size="11" fill="#475569">${algo.category === "specialized_float" ? i18n.type_specialized : i18n.type_general}</text>
      ${highlightBadge}

      <!-- Decode Throughput Column (Sapphire Blue Bar) -->
      <g transform="translate(250, ${y + 5})">
        <rect width="${barW}" height="14" rx="3" fill="#dbeafe"/>
        <rect width="${decBarLen}" height="14" rx="3" fill="${c.dec}"/>
        <text x="${barW + 10}" y="12" font-size="12.5" font-weight="${isFastalp ? "bold" : "600"}" fill="${c.text}">${decSpeed.toFixed(2)} GB/s</text>
      </g>

      <!-- Encode Throughput Column (Warm Amber Gold Bar) -->
      <g transform="translate(500, ${y + 5})">
        <rect width="${barW}" height="14" rx="3" fill="#fef3c7"/>
        <rect width="${encBarLen}" height="14" rx="3" fill="${c.enc}"/>
        <text x="${barW + 10}" y="12" font-size="12.5" font-weight="${isFastalp ? "bold" : "600"}" fill="${c.encText || "#92400e"}">${encSpeed.toFixed(2)} GB/s</text>
      </g>

      <!-- Compression Ratio (Direct x, Dark Text) -->
      <text x="760" y="${y + 20}" font-size="14" font-weight="${isFastalp ? "bold" : "600"}" fill="${isFastalp ? "#1d4ed8" : "#0f172a"}">${ratio.toFixed(2)}x</text>

      <!-- Total Compressed Size -->
      <text x="910" y="${y + 20}" font-size="13.5" font-weight="600" fill="#0f172a">${totalKB} KB</text>

      <!-- Comparison Status -->
      ${vsBase}
    </g>`;
  }).join("\n");

  // 2. Prepare Section 2 datasets (All 31 Datasets in Shared Unified Card)
  const fDatasets = fastalpAlgo.paper_31?.datasets || [];
  const cDatasetsMap = new Map((cppAlgo.paper_31?.datasets || []).map(d => [d.name, d]));

  const renderSubtable = (datasetsSubset, startX, startY, isRightCol = false) => {
    const tableX = startX;
    const tableY = startY;

    const rows = datasetsSubset.map((d, rIdx) => {
      const cy = tableY + dsHeaderH + 14 + rIdx * dsRowH;
      const c = cDatasetsMap.get(d.name) || {};
      const fRatio = d.ratio.toFixed(2);
      const cRatio = (c.ratio ?? 0).toFixed(2);
      const fDec = d.dec_gb_s ? d.dec_gb_s.toFixed(1) : "-";
      const cDec = c.dec_gb_s ? c.dec_gb_s.toFixed(1) : "-";
      
      const speedupNum = (d.dec_gb_s && c.dec_gb_s) ? (d.dec_gb_s / c.dec_gb_s) : 1.0;
      const isFaster = speedupNum >= 1.05;
      const speedupStr = speedupNum >= 1.05 ? `+${((speedupNum - 1) * 100).toFixed(0)}%` : speedupNum.toFixed(2) + "x";

      const bg = rIdx % 2 === 1 ? `<rect x="${tableX}" y="${cy - 13}" width="${subW}" height="${dsRowH - 2}" rx="4" fill="#f8fafc"/>` : "";

      return `
      ${bg}
      <text x="${tableX + 12}" y="${cy + 3}" font-size="10.5" font-weight="600" fill="#0f172a">${xmlEscape(d.name)}</text>
      <text x="${tableX + 180}" y="${cy + 3}" font-size="11" font-weight="bold" fill="#1d4ed8">${fRatio}x</text>
      <text x="${tableX + 255}" y="${cy + 3}" font-size="11" font-weight="500" fill="#475569">${cRatio}x</text>
      <text x="${tableX + 330}" y="${cy + 3}" font-size="11" font-weight="bold" fill="#1d4ed8">${fDec} GB/s</text>
      <text x="${tableX + 415}" y="${cy + 3}" font-size="11" font-weight="500" fill="#475569">${cDec} GB/s</text>
      <text x="${tableX + subW - 12}" y="${cy + 3}" font-size="10.5" font-weight="bold" fill="${isFaster ? "#1e3a8a" : "#64748b"}" text-anchor="end">${speedupStr}</text>
      `;
    }).join("");

    // If right col, add summary row
    const summaryRow = isRightCol ? `
      <rect x="${tableX}" y="${tableY + dsHeaderH + 14 + 15 * dsRowH - 13}" width="${subW}" height="${dsRowH - 2}" rx="4" fill="#eff6ff"/>
      <text x="${tableX + 12}" y="${tableY + dsHeaderH + 14 + 15 * dsRowH + 3}" font-size="11" font-weight="bold" fill="#1d4ed8">${isZh ? "31 集全量综合平均" : "31 Datasets Average"}</text>
      <text x="${tableX + 180}" y="${tableY + dsHeaderH + 14 + 15 * dsRowH + 3}" font-size="11" font-weight="bold" fill="#1d4ed8">${fastalpAlgo.paper_31.ratio.toFixed(2)}x</text>
      <text x="${tableX + 255}" y="${tableY + dsHeaderH + 14 + 15 * dsRowH + 3}" font-size="11" font-weight="bold" fill="#475569">${cppAlgo.paper_31.ratio.toFixed(2)}x</text>
      <text x="${tableX + 330}" y="${tableY + dsHeaderH + 14 + 15 * dsRowH + 3}" font-size="11" font-weight="bold" fill="#1d4ed8">${fastalpAlgo.paper_31.avg_dec_gb_s.toFixed(1)} GB/s</text>
      <text x="${tableX + 415}" y="${tableY + dsHeaderH + 14 + 15 * dsRowH + 3}" font-size="11" font-weight="bold" fill="#475569">${cppAlgo.paper_31.avg_dec_gb_s.toFixed(1)} GB/s</text>
      <text x="${tableX + subW - 12}" y="${tableY + dsHeaderH + 14 + 15 * dsRowH + 3}" font-size="11" font-weight="bold" fill="#1e3a8a" text-anchor="end">${isZh ? "+10.1% 提速" : "+10.1% Speed"}</text>
    ` : "";

    return `
    <g class="subtable">
      <!-- Subtable Header with generous breathing margins -->
      <rect x="${tableX}" y="${tableY}" width="${subW}" height="${dsHeaderH}" rx="5" fill="#f1f5f9"/>
      <text x="${tableX + 12}" y="${tableY + 20}" font-size="10" font-weight="bold" fill="#334155">${i18n.col_dataset}</text>
      <text x="${tableX + 180}" y="${tableY + 20}" font-size="10" font-weight="bold" fill="#1d4ed8">${i18n.col_f_ratio}</text>
      <text x="${tableX + 255}" y="${tableY + 20}" font-size="10" font-weight="bold" fill="#475569">${i18n.col_c_ratio}</text>
      <text x="${tableX + 330}" y="${tableY + 20}" font-size="10" font-weight="bold" fill="#1d4ed8">${i18n.col_f_dec}</text>
      <text x="${tableX + 415}" y="${tableY + 20}" font-size="10" font-weight="bold" fill="#475569">${i18n.col_c_dec}</text>
      <text x="${tableX + subW - 12}" y="${tableY + 20}" font-size="10" font-weight="bold" fill="#334155" text-anchor="end">${i18n.col_speedup}</text>

      ${rows}
      ${summaryRow}
    </g>
    `;
  };

  const leftDatasets = fDatasets.slice(0, 16);
  const rightDatasets = fDatasets.slice(16, 31);
  const sec2SubtablesY = sec2Y + 48 + 24; // 24px margin below shared header strip
  const sec2LeftSvg = renderSubtable(leftDatasets, 32 + 16, sec2SubtablesY, false);
  const sec2RightSvg = renderSubtable(rightDatasets, 32 + 16 + subW + 16, sec2SubtablesY, true);

  // 3. Section 3: 4 Scenarios Microbenchmarks (2x2 Grid)
  const scenariosData = [
    {
      title: i18n.scene_sensor_title,
      sub: i18n.scene_sensor_sub,
      badge: i18n.scene_sensor_badge,
      badgeW: isZh ? 76 : 84,
      items: [
        { name: "fastalp (Rust)", dec: "22.8 GB/s", enc: "4.8 GB/s", ratio: "7.91x", vs: isZh ? "领先 (零误差)" : "Leader (Exact)", bold: true, color: "#1d4ed8" },
        { name: "C++ ALP",       dec: "21.8 GB/s", enc: "0.8 GB/s", ratio: "7.86x", vs: isZh ? "基准 (压缩慢)" : "Baseline (Slow)", bold: false, color: "#1e293b" },
        { name: "Pcodec (pco)",  dec: "1.58 GB/s", enc: "0.2 GB/s", ratio: "7.82x", vs: isZh ? "解压仅 0.07x" : "0.07x Decode", bold: false, color: "#475569" },
        { name: "LZ4",           dec: "11.3 GB/s", enc: "3.8 GB/s", ratio: "1.25x", vs: isZh ? "压缩率仅 1.25x" : "1.25x Ratio", bold: false, color: "#475569" },
      ]
    },
    {
      title: i18n.scene_ramp_title,
      sub: i18n.scene_ramp_sub,
      badge: i18n.scene_ramp_badge,
      badgeW: isZh ? 76 : 84,
      items: [
        { name: "fastalp (Delta)",dec: "40.7 GB/s", enc: "3.1 GB/s", ratio: "431.2x", vs: isZh ? "比 C++ 高 458x" : "458x vs C++", bold: true, color: "#1d4ed8" },
        { name: "Pcodec (pco)",  dec: "0.85 GB/s", enc: "0.1 GB/s", ratio: "44.5x", vs: isZh ? "体积大 9.7x" : "9.7x Size", bold: false, color: "#475569" },
        { name: "Zstd (Level 3)", dec: "0.90 GB/s", enc: "0.3 GB/s", ratio: "6.93x", vs: isZh ? "解压慢 45x" : "45x Decode", bold: false, color: "#475569" },
        { name: "C++ ALP",       dec: "0.58 GB/s", enc: "0.4 GB/s", ratio: "0.94x", vs: isZh ? "异常负膨胀" : "Negative Exp", bold: false, color: "#991b1b" },
      ]
    },
    {
      title: i18n.scene_stock_title,
      sub: i18n.scene_stock_sub,
      badge: i18n.scene_stock_badge,
      badgeW: isZh ? 76 : 84,
      items: [
        { name: "fastalp (Rust)", dec: "24.2 GB/s", enc: "4.2 GB/s", ratio: "10.41x", vs: isZh ? "解压快 14%" : "+14% Decode", bold: true, color: "#1d4ed8" },
        { name: "C++ ALP",       dec: "21.2 GB/s", enc: "1.2 GB/s", ratio: "4.19x", vs: isZh ? "压缩率仅 4.19x" : "4.19x Ratio", bold: false, color: "#1e293b" },
        { name: "Snappy (snap)", dec: "5.62 GB/s", enc: "2.8 GB/s", ratio: "1.28x", vs: isZh ? "压缩率仅 1.28x" : "1.28x Ratio", bold: false, color: "#475569" },
        { name: "Gorilla (XOR)", dec: "0.34 GB/s", enc: "0.3 GB/s", ratio: "2.12x", vs: isZh ? "解压慢 71x" : "71x Decode", bold: false, color: "#475569" },
      ]
    },
    {
      title: i18n.scene_const_title,
      sub: i18n.scene_const_sub,
      badge: i18n.scene_const_badge,
      badgeW: isZh ? 76 : 84,
      items: [
        { name: "fastalp (Rust)", dec: "88.9 GB/s", enc: "24.6 GB/s", ratio: "744.7x", vs: isZh ? "比 C++ 快 3.8x" : "3.8x vs C++", bold: true, color: "#1d4ed8" },
        { name: "C++ ALP",       dec: "23.5 GB/s", enc: "7.2 GB/s", ratio: "455.1x", vs: isZh ? "基准" : "Baseline", bold: false, color: "#1e293b" },
        { name: "Zstd (Level 3)", dec: "3.37 GB/s", enc: "3.5 GB/s", ratio: "292.6x", vs: isZh ? "解压慢 26x" : "26x Decode", bold: false, color: "#475569" },
        { name: "Pcodec (pco)",  dec: "4.82 GB/s", enc: "0.5 GB/s", ratio: "282.5x", vs: isZh ? "解压慢 18x" : "18x Decode", bold: false, color: "#475569" },
      ]
    }
  ];

  const sec3Svg = scenariosData.map((sc, sIdx) => {
    const row = Math.floor(sIdx / 2);
    const col = sIdx % 2;
    const sx = 32 + col * (scW + 24);
    const sy = sec3Y + 44 + row * (scH + 18);

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
    <text x="16" y="44" font-size="11.5" font-weight="500" fill="#1e293b">${i18n.env_cpu}</text>
    <text x="16" y="64" font-size="11" fill="#475569">${i18n.env_toolchain}</text>
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
    <text x="250" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_dec}</text>
    <text x="500" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_enc}</text>
    <text x="760" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_ratio}</text>
    <text x="910" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_size}</text>
    <text x="1110" y="${sec1HeaderY + 22}" font-size="11" font-weight="bold" fill="#1e293b">${i18n.col_status}</text>

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
    <text x="36" y="${sec3Y - 14}" font-size="16" font-weight="bold" fill="#0f172a">${i18n.sec3_title}</text>
    <text x="36" y="${sec3Y + 6}" font-size="12" fill="#475569">${i18n.sec3_sub}</text>

    ${sec3Svg}
  </g>

  <!-- Footer (Centered, NO dividing line above) -->
  <text x="${width / 2}" y="${totalH - 24}" font-size="11.5" font-weight="500" fill="#475569" text-anchor="middle">${i18n.footer_left}</text>
</svg>
`;
};

export default renderSvg;
