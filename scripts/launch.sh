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
if [ $# -ne 2 ]; then
  echo "Uso: $0 <N_replicas> <M_estaciones>"
  exit 1
fi

N_REPLICAS=$1
M_ESTACIONES=$2

BASE_PORT=9000
BASE_LAT=-34.60
BASE_LON=-58.38

# --- Compilar el servidor si no existe ---
if [ ! -f "$SERVER_BIN" ]; then
  echo "[BUILD] Compilando servidor..."
  cargo build --manifest-path "$SERVER_CARGO"
fi

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
LEADER_PORT=$((BASE_PORT))
LEADER_IP="127.0.0.1"
LEADER_CMD="$SERVER_BIN \
  --role leader \
  --ip $LEADER_IP \
  --port $LEADER_PORT \
  --coords $BASE_LAT $BASE_LON \
  --max-conns 32"

echo "[BOOT] Lanzando líder en puerto $LEADER_PORT"
open_term "$LEADER_CMD" "Líder"

# --- Lanzar réplicas ---
for i in $(seq 1 $N_REPLICAS); do
  PORT=$((BASE_PORT + i))
  LAT=$(echo "$BASE_LAT - 0.01 * $i" | bc)
  LON=$(echo "$BASE_LON - 0.01 * $i" | bc)
  CMD="$SERVER_BIN \
    --role replica \
    --ip 127.0.0.1 \
    --port $PORT \
    --coords $LAT $LON \
    --leader 127.0.0.1:$LEADER_PORT"
  echo "[BOOT] Lanzando réplica $i en puerto $PORT"
  open_term "$CMD" "Réplica $i"
done

# --- Lanzar estaciones ---
for j in $(seq 1 $M_ESTACIONES); do
  PORT=$((BASE_PORT + N_REPLICAS + j))
  LAT=$(echo "$BASE_LAT + 0.02 * $j" | bc)
  LON=$(echo "$BASE_LON + 0.02 * $j" | bc)
  CMD="$SERVER_BIN \
    --role station \
    --ip 127.0.0.1 \
    --port $PORT \
    --coords $LAT $LON \
    --leader 127.0.0.1:$LEADER_PORT"
  echo "[BOOT] Lanzando estación $j en puerto $PORT"
  open_term "$CMD" "Estación $j"
done

echo ""
echo "✅ Sistema YPF Ruta iniciado con:"
echo "   - 1 Líder en puerto $LEADER_PORT"
echo "   - $N_REPLICAS Réplicas"
echo "   - $M_ESTACIONES Estaciones"
echo ""
