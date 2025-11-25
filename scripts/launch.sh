#!/usr/bin/env bash
# ======================================================
#  YPF Ruta Distributed Node Launcher
# ======================================================

set -e

# --- Detectar raíz del proyecto ---
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
SERVER_BIN="$ROOT_DIR/server/target/debug/server"
SERVER_CARGO="$ROOT_DIR/server/Cargo.toml"

# --- Validación de argumentos ---
if [ $# -ne 1 ]; then
  echo "Uso: $0 <N_replicas>"
  echo "Ejemplo: $0 3"
  exit 1
fi

N_REPLICAS=$1

BASE_PORT=5000
BASE_LAT=-34.60
BASE_LON=-58.38

# --- Función para abrir terminales ---
open_term() {
  local CMD=$1
  local TITLE=$2
  local WORKDIR=$ROOT_DIR
  if command -v gnome-terminal &>/dev/null; then
    gnome-terminal -- bash -c "cd '$WORKDIR'; $CMD; exec bash" --title="$TITLE"
  elif command -v xterm &>/dev/null; then
    xterm -T "$TITLE" -e "cd '$WORKDIR'; $CMD"
  elif command -v konsole &>/dev/null; then
    konsole --new-tab -p tabtitle="$TITLE" -e bash -c "cd '$WORKDIR'; $CMD; exec bash"
  else
    echo "[WARN] No se encontró terminal compatible, ejecutando en background"
    bash -c "cd '$WORKDIR'; $CMD" &
  fi
}

# --- Lanzar líder ---
LEADER_PORT=$BASE_PORT
LEADER_IP="127.0.0.1"
LEADER_ADDR="$LEADER_IP:$LEADER_PORT"
LEADER_CMD="cargo run --bin server -- \
  --address $LEADER_ADDR \
  --pumps 4 \
  leader \
  --max-conns 32"

echo "[BOOT] Lanzando líder en $LEADER_ADDR"
open_term "$LEADER_CMD" "Líder ($LEADER_PORT)"

# Esperar un poco para que el líder esté listo
sleep 2

# --- Lanzar réplicas ---
for i in $(seq 1 $N_REPLICAS); do
  PORT=$((BASE_PORT + i))
  LAT=$(echo "$BASE_LAT - 0.01 * $i" | bc)
  LON=$(echo "$BASE_LON - 0.01 * $i" | bc)
  REPLICA_ADDR="127.0.0.1:$PORT"
  
  CMD="cargo run --bin server -- \
    --address $REPLICA_ADDR \
    --pumps 4 \
    replica \
    --leader-addr $LEADER_ADDR \
    --max-conns 32"
  
  echo "[BOOT] Lanzando réplica $i en $REPLICA_ADDR"
  open_term "$CMD" "Réplica $i ($PORT)"
  
  # Pequeña pausa entre réplicas para evitar race conditions
  sleep 0.5
done

echo ""
echo "✅ Sistema YPF Ruta iniciado con:"
echo "   - 1 Líder en $LEADER_ADDR"
echo "   - $N_REPLICAS Réplicas (puertos $((BASE_PORT + 1))-$((BASE_PORT + N_REPLICAS)))"
echo ""
