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
  const { algorithms, summary } = benchData;

  const i18n = Object.fromEntries(
    Object.entries(rawI18n).map(([k, v]) => [k, typeof v === "string" ? xmlEscape(v) : v])
  );

  const width = 1200;

  // Geometry: spacious and breathable layout
  const headerH = 130;
  const cardY = headerH + 20;
  const cardH = 108;
  const sec1Y = cardY + cardH + 40;
  const rowH = 62; // generous row height for dual bars
  const sec1H = 56 + algorithms.length * rowH;
  const sec2Y = sec1Y + sec1H + 40;
  const sec2H = 240;
  const totalH = sec2Y + sec2H + 70;

  // Color mapping
  const colorMap = {
    fastalp: { decBar: "#38bdf8", encBar: "#0284c7", text: "#38bdf8", badge: "#0284c7" },
    cpp_alp: { decBar: "#34d399", encBar: "#059669", text: "#34d399", badge: "#059669" },
    pco:     { decBar: "#fbbf24", encBar: "#d97706", text: "#fbbf24", badge: "#d97706" },
    zstd:    { decBar: "#a78bfa", encBar: "#7c3aed", text: "#a78bfa", badge: "#7c3aed" },
    lz4:     { decBar: "#f472b6", encBar: "#db2777", text: "#f472b6", badge: "#db2777" },
    snappy:  { decBar: "#818cf8", encBar: "#4f46e5", text: "#818cf8", badge: "#4f46e5" },
    chimp128:{ decBar: "#94a3b8", encBar: "#64748b", text: "#94a3b8", badge: "#475569" },
    gorilla: { decBar: "#64748b", encBar: "#475569", text: "#64748b", badge: "#334155" },
  };

  const maxDecSpeed = 28.0; // scale max for 31-dataset avg decode speed
  const maxEncSpeed = 10.0; // scale max for encode speed
  const barMaxW = 280;

  // Section 1: Main benchmark rows with dual bars (Decompress & Compress)
  const rowsSvg = algorithms.map((algo, idx) => {
    const y = sec1Y + 58 + idx * rowH;
    const c = colorMap[algo.algorithm] || colorMap.gorilla;
    const isFastalp = algo.algorithm === "fastalp";

    const decSpeed = algo.paper_31.avg_dec_gb_s;
    const encSpeed = algo.paper_31.avg_enc_gb_s;
    const decBarW = Math.max(8, Math.min(barMaxW, (decSpeed / maxDecSpeed) * barMaxW));
    const encBarW = Math.max(6, Math.min(barMaxW, (encSpeed / maxEncSpeed) * barMaxW));

    const ratio = algo.paper_31.ratio;
    const bits = algo.paper_31.bits_per_val;
    const totalKB = (algo.paper_31.total_compressed_bytes / 1024).toFixed(1);

    const rowBg = isFastalp
      ? `<rect x="24" y="${y - 8}" width="${width - 48}" height="${rowH - 6}" rx="10" fill="rgba(14, 165, 233, 0.08)" stroke="rgba(56, 189, 248, 0.4)" stroke-width="1.2"/>`
      : `<rect x="24" y="${y - 8}" width="${width - 48}" height="${rowH - 6}" rx="10" fill="rgba(30, 41, 59, 0.35)" stroke="rgba(255, 255, 255, 0.04)" stroke-width="1"/>`;

    const highlightBadge = isFastalp
      ? `<rect x="186" y="${y + 2}" width="58" height="18" rx="4" fill="#0284c7"/><text x="215" y="${y + 15}" font-size="10.5" font-weight="bold" fill="#ffffff" text-anchor="middle">LEADER</text>`
      : "";

    return `
    <g class="algo-row">
      ${rowBg}
      
      <!-- Algo Name & Badge -->
      <text x="44" y="${y + 18}" font-size="14.5" font-weight="${isFastalp ? "bold" : "600"}" fill="${isFastalp ? "#38bdf8" : "#f1f5f9"}">${xmlEscape(algo.display_name)}</text>
      <text x="44" y="${y + 36}" font-size="11.5" fill="#64748b">${algo.category === "specialized_float" ? i18n.type_specialized : i18n.type_general}</text>
      ${highlightBadge}

      <!-- Decompress Bar (Top) -->
      <g transform="translate(265, ${y - 2})">
        <rect width="${barMaxW}" height="14" rx="3" fill="rgba(15, 23, 42, 0.5)"/>
        <rect width="${decBarW}" height="14" rx="3" fill="${c.decBar}"/>
        <text x="${barMaxW + 10}" y="12" font-size="12" font-weight="${isFastalp ? "bold" : "500"}" fill="${c.text}">${decSpeed.toFixed(2)} GB/s</text>
      </g>

      <!-- Compress Bar (Bottom) -->
      <g transform="translate(265, ${y + 20})">
        <rect width="${barMaxW}" height="12" rx="3" fill="rgba(15, 23, 42, 0.4)"/>
        <rect width="${encBarW}" height="12" rx="3" fill="${c.encBar}" opacity="0.85"/>
        <text x="${barMaxW + 10}" y="10" font-size="11" fill="#94a3b8">${encSpeed.toFixed(2)} GB/s</text>
      </g>

      <!-- Compression Ratio & Bits -->
      <text x="730" y="${y + 18}" font-size="14.5" font-weight="${isFastalp ? "bold" : "600"}" fill="${isFastalp ? "#38bdf8" : "#f8fafc"}">${ratio.toFixed(2)}x</text>
      <text x="730" y="${y + 36}" font-size="12" fill="#94a3b8">${bits.toFixed(1)} b/v</text>

      <!-- Total Size -->
      <text x="910" y="${y + 20}" font-size="14" font-weight="500" fill="#e2e8f0">${totalKB} KB</text>
      <text x="910" y="${y + 36}" font-size="11" fill="#64748b">${algo.paper_31.total_compressed_bytes.toLocaleString()} B</text>

      <!-- Advantage / Speedup Tag -->
      <text x="1060" y="${y + 24}" font-size="12.5" font-weight="${isFastalp ? "bold" : "500"}" fill="${isFastalp ? "#34d399" : "#94a3b8"}">
        ${isFastalp ? "+10.1% vs C++" : algo.algorithm === "cpp_alp" ? "Baseline" : (algo.paper_31.avg_dec_gb_s / algorithms[0].paper_31.avg_dec_gb_s).toFixed(2) + "x"}
      </text>
    </g>`;
  }).join("\n");

  // Section 2: Scenarios cards
  const scW = (width - 48 - 36) / 3;
  const scY = sec2Y + 54;
  const scH = 175;

  const scenariosData = [
    {
      title: i18n.scene_sensor,
      sub: "1024 pts (20.0 ~ 34.9)",
      badge: "Decimal Telemetry",
      items: [
        { name: "fastalp", val: "7.91x (8.1 b/v)", dec: "22.6 GB/s", enc: "4.6 GB/s", color: "#38bdf8", bold: true },
        { name: "C++ ALP", val: "7.86x (8.1 b/v)", dec: "21.8 GB/s", enc: "0.8 GB/s", color: "#34d399" },
        { name: "Pcodec", val: "99.9x (0.6 b/v)", dec: "1.58 GB/s", enc: "0.2 GB/s", color: "#fbbf24" },
        { name: "LZ4", val: "12.2x (5.2 b/v)", dec: "11.3 GB/s", enc: "3.8 GB/s", color: "#f472b6" },
      ]
    },
    {
      title: i18n.scene_ramp,
      sub: "1024 pts (100.0 → 151.2)",
      badge: "Delta-ALP Dominance",
      items: [
        { name: "fastalp", val: "431.2x (0.15 b/v)", dec: "28.1 GB/s", enc: "3.0 GB/s", color: "#38bdf8", bold: true },
        { name: "Pcodec", val: "44.5x (1.4 b/v)", dec: "0.85 GB/s", enc: "0.1 GB/s", color: "#fbbf24" },
        { name: "Zstd", val: "6.93x (9.2 b/v)", dec: "0.90 GB/s", enc: "0.3 GB/s", color: "#a78bfa" },
        { name: "C++ ALP", val: "0.94x (Negative)", dec: "0.58 GB/s", enc: "0.5 GB/s", color: "#ef4444" },
      ]
    },
    {
      title: i18n.scene_constant,
      sub: "1024 pts (98.6 identical)",
      badge: "Fast Zero-Heap Exit",
      items: [
        { name: "fastalp", val: "744.7x (0.09 b/v)", dec: "45.2 GB/s", enc: "22.1 GB/s", color: "#38bdf8", bold: true },
        { name: "C++ ALP", val: "455.1x (0.14 b/v)", dec: "21.8 GB/s", enc: "7.0 GB/s", color: "#34d399" },
        { name: "Zstd", val: "292.6x (0.22 b/v)", dec: "3.37 GB/s", enc: "3.5 GB/s", color: "#a78bfa" },
        { name: "Pcodec", val: "282.5x (0.23 b/v)", dec: "4.82 GB/s", enc: "0.5 GB/s", color: "#fbbf24" },
      ]
    }
  ];

  const scenariosSvg = scenariosData.map((sc, sIdx) => {
    const sx = 24 + sIdx * (scW + 18);
    const isRamp = sIdx === 1;

    const itemsSvg = sc.items.map((it, itIdx) => {
      const iy = scY + 58 + itIdx * 27;
      return `
      <text x="${sx + 16}" y="${iy}" font-size="12.5" font-weight="${it.bold ? "bold" : "500"}" fill="${it.bold ? "#38bdf8" : "#cbd5e1"}">${xmlEscape(it.name)}</text>
      <text x="${sx + 92}" y="${iy}" font-size="12" font-weight="${it.bold ? "bold" : "600"}" fill="${it.color}">${xmlEscape(it.val)}</text>
      <text x="${sx + scW - 16}" y="${iy}" font-size="11.5" fill="#94a3b8" text-anchor="end">↓${xmlEscape(it.dec)} ｜ ↑${xmlEscape(it.enc)}</text>
      `;
    }).join("");

    return `
    <g class="scenario-card">
      <rect x="${sx}" y="${scY}" width="${scW}" height="${scH}" rx="12" fill="rgba(15, 23, 42, 0.75)" stroke="${isRamp ? "rgba(56, 189, 248, 0.45)" : "rgba(255, 255, 255, 0.08)"}" stroke-width="${isRamp ? "1.5" : "1"}"/>
      <text x="${sx + 16}" y="${scY + 26}" font-size="13.5" font-weight="bold" fill="#f8fafc">${sc.title}</text>
      <rect x="${sx + scW - 118}" y="${scY + 12}" width="102" height="20" rx="4" fill="${isRamp ? "rgba(14, 165, 233, 0.25)" : "rgba(51, 65, 85, 0.5)"}" stroke="${isRamp ? "rgba(56, 189, 248, 0.4)" : "none"}"/>
      <text x="${sx + scW - 67}" y="${scY + 26}" font-size="10" font-weight="bold" fill="${isRamp ? "#38bdf8" : "#94a3b8"}" text-anchor="middle">${xmlEscape(sc.badge)}</text>
      <line x1="${sx + 16}" y1="${scY + 42}" x2="${sx + scW - 16}" y2="${scY + 42}" stroke="rgba(255, 255, 255, 0.06)" stroke-width="1"/>
      ${itemsSvg}
    </g>
    `;
  }).join("\n");

  return `<?xml version="1.0" encoding="UTF-8"?>
<svg width="${width}" height="${totalH}" viewBox="0 0 ${width} ${totalH}" fill="none" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="card_grad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="rgba(30, 41, 59, 0.6)"/>
      <stop offset="100%" stop-color="rgba(15, 23, 42, 0.7)"/>
    </linearGradient>
    <linearGradient id="title_grad" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#38bdf8"/>
      <stop offset="100%" stop-color="#818cf8"/>
    </linearGradient>
    <filter id="card_shadow" x="-5%" y="-5%" width="110%" height="115%">
      <feDropShadow dx="0" dy="4" stdDeviation="6" flood-color="#000000" flood-opacity="0.25"/>
    </filter>
  </defs>

  <style>
    text { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Hiragino Sans GB", "Microsoft YaHei", sans-serif; }
    .algo-row:hover rect { opacity: 0.95; }
  </style>

  <!-- Header -->
  <text x="36" y="56" font-size="28" font-weight="800" fill="url(#title_grad)" letter-spacing="0.5">${i18n.title}</text>
  <text x="36" y="88" font-size="14.5" fill="#94a3b8">${i18n.subtitle}</text>

  <!-- Environment Badge -->
  <g transform="translate(${width - 490}, 30)">
    <rect width="454" height="70" rx="10" fill="rgba(15, 23, 42, 0.75)" stroke="rgba(255, 255, 255, 0.08)" stroke-width="1"/>
    <text x="16" y="22" font-size="11" font-weight="bold" fill="#38bdf8">BENCHMARK ENVIRONMENT</text>
    <text x="16" y="40" font-size="11.5" fill="#cbd5e1">${i18n.env_cpu}</text>
    <text x="16" y="58" font-size="11" fill="#94a3b8">${i18n.env_toolchain} ｜ ${i18n.env_os}</text>
  </g>

  <!-- Top 4 Metric Highlight Cards -->
  <!-- Card 1 -->
  <g transform="translate(24, ${cardY})">
    <rect width="270" height="${cardH}" rx="14" fill="url(#card_grad)" stroke="rgba(56, 189, 248, 0.4)" stroke-width="1.5" filter="url(#card_shadow)"/>
    <text x="18" y="28" font-size="12.5" font-weight="600" fill="#94a3b8">${i18n.card1_title}</text>
    <text x="18" y="64" font-size="30" font-weight="800" fill="#38bdf8">${i18n.card1_val}</text>
    <text x="18" y="88" font-size="11.5" fill="#64748b">${i18n.card1_sub}</text>
  </g>

  <!-- Card 2 -->
  <g transform="translate(318, ${cardY})">
    <rect width="270" height="${cardH}" rx="14" fill="url(#card_grad)" stroke="rgba(52, 211, 153, 0.4)" stroke-width="1.5" filter="url(#card_shadow)"/>
    <text x="18" y="28" font-size="12.5" font-weight="600" fill="#94a3b8">${i18n.card2_title}</text>
    <text x="18" y="64" font-size="30" font-weight="800" fill="#34d399">${i18n.card2_val}</text>
    <text x="18" y="88" font-size="11.5" fill="#64748b">${i18n.card2_sub}</text>
  </g>

  <!-- Card 3 -->
  <g transform="translate(612, ${cardY})">
    <rect width="270" height="${cardH}" rx="14" fill="url(#card_grad)" stroke="rgba(129, 140, 248, 0.4)" stroke-width="1.5" filter="url(#card_shadow)"/>
    <text x="18" y="28" font-size="12.5" font-weight="600" fill="#94a3b8">${i18n.card3_title}</text>
    <text x="18" y="64" font-size="30" font-weight="800" fill="#818cf8">${i18n.card3_val}</text>
    <text x="18" y="88" font-size="11.5" fill="#64748b">${i18n.card3_sub}</text>
  </g>

  <!-- Card 4 -->
  <g transform="translate(906, ${cardY})">
    <rect width="270" height="${cardH}" rx="14" fill="url(#card_grad)" stroke="rgba(244, 114, 182, 0.4)" stroke-width="1.5" filter="url(#card_shadow)"/>
    <text x="18" y="28" font-size="12.5" font-weight="600" fill="#94a3b8">${i18n.card4_title}</text>
    <text x="18" y="64" font-size="30" font-weight="800" fill="#f472b6">${i18n.card4_val}</text>
    <text x="18" y="88" font-size="11.5" fill="#64748b">${i18n.card4_sub}</text>
  </g>

  <!-- Section 1: Main Table / Dual Bars -->
  <g transform="translate(0, 0)">
    <text x="36" y="${sec1Y - 16}" font-size="17" font-weight="bold" fill="#f8fafc">${i18n.sec1_title}</text>
    <text x="36" y="${sec1Y + 6}" font-size="12.5" fill="#94a3b8">${i18n.sec1_sub}</text>

    <!-- Legend -->
    <g transform="translate(${width - 320}, ${sec1Y - 14})">
      <rect width="12" height="12" rx="3" fill="#38bdf8"/>
      <text x="18" y="10" font-size="11.5" fill="#cbd5e1">${i18n.legend_dec_speed}</text>
      <rect x="130" y="0" width="12" height="12" rx="3" fill="#0284c7" opacity="0.85"/>
      <text x="148" y="10" font-size="11.5" fill="#94a3b8">${i18n.legend_enc_speed}</text>
    </g>

    <!-- Table Header Labels -->
    <text x="44" y="${sec1Y + 36}" font-size="12" font-weight="bold" fill="#64748b">${i18n.col_algo.toUpperCase()}</text>
    <text x="265" y="${sec1Y + 36}" font-size="12" font-weight="bold" fill="#64748b">${i18n.col_dec.toUpperCase()} &amp; ${i18n.col_enc.toUpperCase()}</text>
    <text x="730" y="${sec1Y + 36}" font-size="12" font-weight="bold" fill="#64748b">${i18n.col_ratio.toUpperCase()}</text>
    <text x="910" y="${sec1Y + 36}" font-size="12" font-weight="bold" fill="#64748b">${i18n.col_size.toUpperCase()}</text>
    <text x="1060" y="${sec1Y + 36}" font-size="12" font-weight="bold" fill="#64748b">STATUS</text>
    <line x1="24" y1="${sec1Y + 44}" x2="${width - 24}" y2="${sec1Y + 44}" stroke="rgba(255, 255, 255, 0.08)" stroke-width="1"/>

    ${rowsSvg}
  </g>

  <!-- Section 2: Scenarios -->
  <g transform="translate(0, 0)">
    <text x="36" y="${sec2Y - 16}" font-size="17" font-weight="bold" fill="#f8fafc">${i18n.sec2_title}</text>
    <text x="36" y="${sec2Y + 6}" font-size="12.5" fill="#94a3b8">${i18n.sec2_sub}</text>

    ${scenariosSvg}
  </g>

  <!-- Footer -->
  <line x1="24" y1="${totalH - 40}" x2="${width - 24}" y2="${totalH - 40}" stroke="rgba(255, 255, 255, 0.08)" stroke-width="1"/>
  <text x="36" y="${totalH - 18}" font-size="11.5" fill="#64748b">Measured side-by-side with real C++ ALP library on Apple M2 Max. Lossless IEEE 754 bit-exact bit-pattern reconstruction guaranteed.</text>
  <text x="${width - 36}" y="${totalH - 18}" font-size="11.5" fill="#38bdf8" text-anchor="end">fastalp @ crates.io/crates/fastalp</text>
</svg>
`;
};

export default renderSvg;
