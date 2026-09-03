#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_DIR="$DIR/data"
mkdir -p "$DATA_DIR"

YEAR="${YEAR:-2023}"

fetch() {
  local target="$1" url="$2" label="$3"
  if [ -s "$target" ]; then
    echo "have    $label"
    return 0
  fi
  if curl -fsSL --max-time 60 "$url" -o "$target.partial" && [ -s "$target.partial" ]; then
    mv "$target.partial" "$target"
    echo "fetched $label ($(wc -l < "$target") rows)"
  else
    rm -f "$target.partial"
    echo "missing $label" >&2
  fi
}

echo "Fetching NOAA CO-OPS Tide Gauges..."
for gauge in "8724580" "9414290" "8443970" "8518750"; do
  fetch "$DATA_DIR/coops-$gauge-$YEAR.csv" \
    "https://api.tidesandcurrents.noaa.gov/api/prod/datagetter?product=water_level&application=graupel&begin_date=${YEAR}0101&end_date=${YEAR}0131&datum=MLLW&station=$gauge&time_zone=gmt&units=metric&format=csv" \
    "coops $gauge"
done

echo "Fetching USGS River Gauges..."
for site in "01646500" "06934500" "09380000" "14211720"; do
  fetch "$DATA_DIR/usgs-$site-$YEAR.rdb" \
    "https://waterservices.usgs.gov/nwis/iv/?format=rdb&sites=$site&startDT=${YEAR}-01-01&endDT=${YEAR}-01-31&parameterCd=00060,00065" \
    "usgs $site"
done

echo "Fetching NOAA ISD-Lite Weather Stations..."
for station in "080840-99999" "082210-99999" "071500-99999" "037720-99999" "604300-99999" "411940-99999" "486980-99999" "722020-12839" "723650-23050" "947680-99999"; do
  gz="$DATA_DIR/$station-$YEAR.gz"
  target="$DATA_DIR/$station-$YEAR.isd"
  if [ ! -s "$target" ]; then
    if curl -fsSL --max-time 60 "https://www.ncei.noaa.gov/pub/data/noaa/isd-lite/$YEAR/$station-$YEAR.gz" -o "$gz"; then
      gunzip -c "$gz" > "$target" && rm -f "$gz"
      echo "fetched isd $station ($(wc -l < "$target") rows)"
    else
      rm -f "$gz"
      echo "missing isd $station" >&2
    fi
  else
    echo "have    isd $station"
  fi
done

echo "Download completed: $(ls -1 "$DATA_DIR" | wc -l) files in $DATA_DIR"
