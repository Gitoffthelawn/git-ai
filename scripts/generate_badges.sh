#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CONFIG="$SCRIPT_DIR/badge_config.json"
ICONS_DIR="$REPO_ROOT/assets/docs/agents"
OUT_DIR="$REPO_ROOT/assets/docs/badges"

mkdir -p "$OUT_DIR"

height=100
right_width=60
padding=4
radius=12

count=$(jq length "$CONFIG")

for ((i = 0; i < count; i++)); do
  icon=$(jq -r ".[$i].icon" "$CONFIG")
  name=$(jq -r ".[$i].name" "$CONFIG")

  png="$ICONS_DIR/${icon}.png"
  if [[ ! -f "$png" ]]; then
    echo "WARNING: $png not found, skipping $icon"
    continue
  fi

  # Read PNG dimensions and compute icon width to preserve aspect ratio
  read png_w png_h < <(python3 -c "
import struct, zlib
with open('$png','rb') as f:
    f.read(16)
    w, h = struct.unpack('>II', f.read(8))
    print(w, h)
")
  img_height=$(( height - padding * 2 ))
  img_width=$(python3 -c "print(int(round($img_height * $png_w / $png_h)))")
  left_width=$(( img_width + padding * 2 ))
  width=$(( left_width + right_width ))

  # Checkmark dimensions
  check_cx=$(( left_width + right_width / 2 ))
  check_cy=$(( height / 2 ))

  b64=$(base64 < "$png" | tr -d '\n')

  cat > "$OUT_DIR/${icon}.svg" <<SVGEOF
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="${width}" height="${height}">
  <clipPath id="clip-${icon}">
    <rect width="${width}" height="${height}" rx="${radius}" ry="${radius}"/>
  </clipPath>
  <g clip-path="url(#clip-${icon})">
    <rect width="${left_width}" height="${height}" fill="#FFFFFF"/>
    <rect x="${left_width}" width="${right_width}" height="${height}" fill="#22C55E"/>
  </g>
  <image x="${padding}" y="${padding}" width="${img_width}" height="${img_height}" xlink:href="data:image/png;base64,${b64}"/>
  <polyline points="$(( check_cx - 10 )),${check_cy} $(( check_cx - 3 )),$(( check_cy + 10 )) $(( check_cx + 12 )),$(( check_cy - 10 ))" fill="none" stroke="#FFFFFF" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>
  <rect width="${width}" height="${height}" rx="${radius}" ry="${radius}" fill="none" stroke="#708090" stroke-width="2.5"/>
</svg>
SVGEOF

  echo "Generated $OUT_DIR/${icon}.svg"
done

echo "Done. ${count} badges generated in $OUT_DIR"
