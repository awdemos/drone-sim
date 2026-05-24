#!/usr/bin/env bash
set -euo pipefail

# download_osm.sh — Fetch OpenStreetMap building + road data via Overpass API
# Usage: ./scripts/download_osm.sh <lat> <lon> <radius_meters> [outfile]
# Example: ./scripts/download_osm.sh 37.7749 -122.4194 1000

LAT="${1:-37.7749}"
LON="${2:--122.4194}"
RADIUS="${3:-1000}"
OUTFILE="${4:-data/osm/map.json}"

# Convert radius in meters to approximate degrees
# 1 deg lat ≈ 111 km, 1 deg lon ≈ 111 km * cos(lat)
DEG_LAT=$(awk "BEGIN {printf \"%.6f\", $RADIUS / 111320.0}")
DEG_LON=$(awk "BEGIN {printf \"%.6f\", $RADIUS / (111320.0 * cos($LAT * 3.14159265 / 180.0))}")

MIN_LAT=$(awk "BEGIN {printf \"%.6f\", $LAT - $DEG_LAT}")
MAX_LAT=$(awk "BEGIN {printf \"%.6f\", $LAT + $DEG_LAT}")
MIN_LON=$(awk "BEGIN {printf \"%.6f\", $LON - $DEG_LON}")
MAX_LON=$(awk "BEGIN {printf \"%.6f\", $LON + $DEG_LON}")

echo "Downloading OSM data for bbox: $MIN_LAT,$MIN_LON,$MAX_LAT,$MAX_LON"

# Overpass API query — buildings and roads
QUERY='[out:json];
(
  way["building"]('"$MIN_LAT"','"$MIN_LON"','"$MAX_LAT"','"$MAX_LON"');
  way["highway"]('"$MIN_LAT"','"$MIN_LON"','"$MAX_LAT"','"$MAX_LON"');
);
out body;
>;
out skel qt;'

mkdir -p "$(dirname "$OUTFILE")"

echo "Querying Overpass API (this may take a few seconds)..."
curl -s -X POST \
  -H "Content-Type: application/x-www-form-urlencoded" \
  --data-urlencode "data=$QUERY" \
  "https://overpass-api.de/api/interpreter" \
  -o "$OUTFILE"

# Validate we got JSON
if ! head -c 1 "$OUTFILE" | grep -q '{'; then
    echo "ERROR: Overpass API did not return JSON. Response:"
    cat "$OUTFILE"
    rm -f "$OUTFILE"
    exit 1
fi

ELEMENTS=$(grep -o '"elements"' "$OUTFILE" | wc -l)
if [ "$ELEMENTS" -eq 0 ]; then
    echo "WARNING: No elements found in response. The area may be empty or the query failed."
else
    echo "Saved OSM data to $OUTFILE"
    SIZE=$(du -h "$OUTFILE" | cut -f1)
    echo "File size: $SIZE"
fi
