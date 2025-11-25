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

# --- Compilar el servidor si no existe ---
if [ ! -f "$SERVER_BIN" ]; then
  echo "[BUILD] Compilando servidor..."
  cargo build --manifest-path "$SERVER_CARGO"
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
  elif command -v tmux &>/dev/null; then
    # abre un nuevo pane dentro de la sesión `ypf_ruta`
    TMUX_SESSION="${TMUX_SESSION:-ypf_ruta}"

    # Crear sesión y ejecutar el primer comando si no existe
    if ! tmux has-session -t "$TMUX_SESSION" 2>/dev/null; then
      tmux new-session -d -s "$TMUX_SESSION" -c "$WORKDIR" -n main
      tmux send-keys -t "$TMUX_SESSION:0.0" "cd '$WORKDIR' && $CMD" C-m
      tmux select-pane -t "$TMUX_SESSION:0.0" -T "$TITLE" 2>/dev/null || true
    else
      # Crear un nuevo pane vertical, ordenar en tiled y ejecutar comando ahí
      tmux split-window -t "$TMUX_SESSION:0" -c "$WORKDIR" -v
      tmux select-layout -t "$TMUX_SESSION:0" tiled
      LAST_PANE=$(tmux list-panes -t "$TMUX_SESSION:0" -F "#{pane_index}" | tail -n1)
      tmux send-keys -t "$TMUX_SESSION:0.$LAST_PANE" "cd '$WORKDIR' && $CMD" C-m
      tmux select-pane -t "$TMUX_SESSION:0.$LAST_PANE" -T "$TITLE" 2>/dev/null || true
    fi
  else
    echo "[WARN] No se encontró terminal compatible, ejecutando en background"
    bash -c "cd '$WORKDIR'; $CMD" &
  fi
}

# --- Limpiar sesión tmux previa ---
if command -v tmux &>/dev/null; then
  TMUX_SESSION="${TMUX_SESSION:-ypf_ruta}"
  tmux kill-session -t "$TMUX_SESSION" 2>/dev/null || true
fi

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

if command -v tmux &>/dev/null; then
  # run with mouse mode "setw -g mouse on"
  tmux setw -g mouse on
  # run: tmux attach -t ypf_ruta"
  command -v tmux &>/dev/null && tmux attach -t "$TMUX_SESSION"
fi

