#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/runN.sh N
NODES="${1:-3}"

IP="127.0.0.1"
BASE_PORT=9001
MAX_CONNS=32

# Coords base y paso (positivos para que clap no confunda signos)
BASE_LAT=34.60
BASE_LON=58.38
STEP_LAT=0.01
STEP_LON=0.01

command -v gnome-terminal >/dev/null || { echo "[ERROR] gnome-terminal not found"; exit 1; }
command -v cargo >/dev/null || { echo "[ERROR] cargo not found in PATH"; exit 1; }

# Moverse a la raíz del repo (este script vive en scripts/)
cd "$(dirname "$0")/.."

RUN="cargo run -p server"

# Precalcular puertos y coords
ports=()
lats=()
lons=()
for (( i=0; i<NODES; i++ )); do
  ports[i]=$(( BASE_PORT + i ))
  lats[i]=$(awk -v b="$BASE_LAT" -v s="$STEP_LAT" -v k="$i" 'BEGIN{printf "%.5f", b + s*k}')
  lons[i]=$(awk -v b="$BASE_LON" -v s="$STEP_LON" -v k="$i" 'BEGIN{printf "%.5f", b + s*k}')
done

HOLD='; echo; echo "[DONE] Press Enter to close..."; read _'

echo "[RUN] Launching $NODES nodes…"
for (( i=0; i<NODES; i++ )); do
  # Vecinos = todos menos yo, en tripletas ip:port lat lon
  neighbors=()
  for (( j=0; j<NODES; j++ )); do
    if [[ $j -ne $i ]]; then
      neighbors+=("$IP:${ports[j]}" "${lats[j]}" "${lons[j]}")
    fi
  done

  CMD="$RUN -- \
    --ip $IP --port ${ports[i]} \
    --coords ${lats[i]} ${lons[i]} \
    --max-conns $MAX_CONNS \
    --neighbors ${neighbors[*]}"

  gnome-terminal \
    --title="YPF Node $((i+1)) (:${ports[i]})" \
    -- bash -lc "$CMD$HOLD" &
done

wait || true
echo "[OK] All nodes launched."
