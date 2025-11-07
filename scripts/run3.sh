#!/usr/bin/env bash
set -euo pipefail

# Config simple (sin negativos)
IP="127.0.0.1"
P1=9001; P2=9002; P3=9003
C1_LAT=34.60; C1_LON=58.38
C2_LAT=34.61; C2_LON=58.39
C3_LAT=34.62; C3_LON=58.40

# Comandos (sin --neighbors -- ni comillas)
CMD_NODE1="cargo run -p server -- \
  --ip $IP --port $P1 \
  --coords $C1_LAT $C1_LON \
  --max-conns 32 \
  --neighbors \
    $IP:$P2 $C2_LAT $C2_LON \
    $IP:$P3 $C3_LAT $C3_LON"

CMD_NODE2="cargo run -p server -- \
  --ip $IP --port $P2 \
  --coords $C2_LAT $C2_LON \
  --max-conns 32 \
  --neighbors \
    $IP:$P1 $C1_LAT $C1_LON \
    $IP:$P3 $C3_LAT $C3_LON"

CMD_NODE3="cargo run -p server -- \
  --ip $IP --port $P3 \
  --coords $C3_LAT $C3_LON \
  --max-conns 32 \
  --neighbors \
    $IP:$P1 $C1_LAT $C1_LON \
    $IP:$P2 $C2_LAT $C2_LON"

HOLD='; echo; echo "[DONE] Press Enter to close..."; read _'

# Lanzar 3 terminales
gnome-terminal --title="YPF Node 1 (:$P1)" -- bash -lc "$CMD_NODE1$HOLD" &
gnome-terminal --title="YPF Node 2 (:$P2)" -- bash -lc "$CMD_NODE2$HOLD" &
gnome-terminal --title="YPF Node 3 (:$P3)" -- bash -lc "$CMD_NODE3$HOLD" &

wait || true
