import { useEffect } from "react";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import "./App.css";
import nodesConfig from "../../../nodes.json";

// =====================
// TIPOS
// =====================
type BackendRole = "leader" | "replica" | "client" | "disconnected";
type NodeRole = BackendRole | "unknown";

type NodeConfig = {
  host: string;
  port: number;
  name?: string;
  lat: number;
  lng: number;
};

type RoleQueryResult = {
  host: string;
  port: number;
  ok: boolean;
  nodeId?: string;
  role?: unknown;
  error?: string;
};

type RolesApiResponse = {
  ok: boolean;
  results: RoleQueryResult[];
};

const NODES: NodeConfig[] = nodesConfig as NodeConfig[];

// =====================
// ICONOS
// =====================
const iconLeader = L.icon({
  iconUrl:
    "https://raw.githubusercontent.com/pointhi/leaflet-color-markers/master/img/marker-icon-red.png",
  iconSize: [38, 55],
  iconAnchor: [19, 55],
});

const iconReplica = L.icon({
  iconUrl:
    "https://raw.githubusercontent.com/pointhi/leaflet-color-markers/master/img/marker-icon-blue.png",
  iconSize: [30, 45],
  iconAnchor: [15, 45],
});

const iconClient = L.icon({
  iconUrl:
    "https://raw.githubusercontent.com/pointhi/leaflet-color-markers/master/img/marker-icon-green.png",
  iconSize: [26, 38],
  iconAnchor: [13, 38],
});

const iconDisconnected = L.icon({
  iconUrl:
    "https://raw.githubusercontent.com/pointhi/leaflet-color-markers/master/img/marker-icon-grey.png",
  iconSize: [26, 38],
  iconAnchor: [13, 38],
});

const iconUnknown = L.icon({
  iconUrl:
    "https://raw.githubusercontent.com/pointhi/leaflet-color-markers/master/img/marker-icon-gold.png",
  iconSize: [26, 38],
  iconAnchor: [13, 38],
});

// =====================
// Conversión role number → string
// =====================
function normalizeRole(role: any): NodeRole {
  if (role === 0) return "leader";
  if (role === 1) return "replica";
  if (role === 2) return "client";
  if (role === "leader" || role === "replica" || role === "client")
    return role;
  return "unknown";
}

function iconForRole(role: NodeRole): L.Icon {
  switch (role) {
    case "leader":
      return iconLeader;
    case "replica":
      return iconReplica;
    case "client":
      return iconClient;
    case "disconnected":
      return iconDisconnected;
    default:
      return iconUnknown;
  }
}

// =====================
// FRONTEND COMPONENT
// =====================
function App() {
  useEffect(() => {
    // Crear mapa full-screen
    const map = L.map("map").setView([-38, -64], 5);

    L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
      attribution: "© OpenStreetMap contributors",
    }).addTo(map);

    const markersByKey: Record<string, L.Marker> = {};
    const nodeByKey: Record<string, NodeConfig> = {};
    const baseIconByKey: Record<string, L.Icon> = {};
    const arrowsLayer = L.layerGroup().addTo(map);

    const keyForNode = (n: { host: string; port: number }) =>
      `${n.host}:${n.port}`;

    // =====================
    // Crear marcadores
    // =====================
    NODES.forEach((node) => {
      const key = keyForNode(node);
      nodeByKey[key] = node;

      const marker = L.marker([node.lat, node.lng], {
        icon: iconUnknown,
      }).addTo(map);

      markersByKey[key] = marker;
      baseIconByKey[key] = iconUnknown;

      const name = node.name ?? key;

      marker.bindPopup(`
        <b>${name}</b><br>
        ID: <b>-</b><br>
        ROL: <b>UNKNOWN</b><br>
        ESTADO: CONSULTANDO…
      `);

      // Hover: agrandar
      marker.on("mouseover", () => {
        const base = baseIconByKey[key];
        const size = base.options.iconSize as [number, number];
        const anchor = base.options.iconAnchor as [number, number];

        marker.setIcon(
          L.icon({
            iconUrl: base.options.iconUrl!,
            iconSize: [size[0] * 1.4, size[1] * 1.4],
            iconAnchor: [anchor[0] * 1.4, anchor[1] * 1.4],
          })
        );
      });

      marker.on("mouseout", () => {
        marker.setIcon(baseIconByKey[key]);
      });
    });

    // =====================
    // Centrar mapa en todos los nodos
    // =====================
    if (NODES.length > 0) {
      const lats = NODES.map((n) => n.lat);
      const lngs = NODES.map((n) => n.lng);

      const bounds = L.latLngBounds(
        [Math.min(...lats), Math.min(...lngs)],
        [Math.max(...lats), Math.max(...lngs)]
      );

      map.fitBounds(bounds.pad(0.2));
    }

    let isFetching = false;

    // =====================
    // Fetch roles (backend)
    // =====================
    async function fetchRolesAndUpdate() {
      if (isFetching) return;
      isFetching = true;

      const btn = document.getElementById(
        "refresh-map"
      ) as HTMLButtonElement | null;
      if (btn) {
        btn.disabled = true;
        btn.innerHTML = `<span class="spinner"></span>Cargando...`;
      }

      try {
        console.log("🌍 Fetching roles from backend…");

        const res = await fetch("http://localhost:3001/api/roles", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            nodes: NODES.map((n) => ({ host: n.host, port: n.port })),
          }),
        });

        console.log("📨 Raw response:", res);

        const json = (await res.json()) as RolesApiResponse;
        console.log("📦 Parsed /api/roles JSON:", json);

        if (!json.ok) {
          console.error("❌ Backend returned ok=false", json);
          return;
        }

        arrowsLayer.clearLayers();

        // =====================
        // Actualizar marcadores
        // =====================
        json.results.forEach((result) => {
          const key = keyForNode(result);
          const marker = markersByKey[key];
          const cfg = nodeByKey[key];
          if (!marker || !cfg) return;

          const role = !result.ok
            ? "disconnected"
            : normalizeRole(result.role);

          const nodeIdText = result.nodeId ?? "-";

          const icon = iconForRole(role);
          marker.setIcon(icon);
          baseIconByKey[key] = icon;

          const name = cfg.name ?? key;

          const estado = result.ok ? "ONLINE" : "DISCONNECTED";

          marker.bindPopup(`
            <b>${name}</b><br>
            ID: <b>${nodeIdText}</b><br>
            ROL: <b>${role.toUpperCase()}</b><br>
            ESTADO: ${estado}${result.error ? `<br><small style="color:red">${result.error}</small>` : ""}
          `);
        });

        // =====================
        // Dibujar flechas leader → replicas
        // =====================
        const leader = json.results.find(
          (r) => r.ok && normalizeRole(r.role) === "leader"
        );

        if (leader) {
          const leaderCfg = nodeByKey[keyForNode(leader)];
          if (leaderCfg) {
            const leaderPoint = [
              leaderCfg.lat,
              leaderCfg.lng,
            ] as [number, number];

            json.results
              .filter((r) => r.ok && normalizeRole(r.role) === "replica")
              .forEach((rep) => {
                const cfg = nodeByKey[keyForNode(rep)];
                if (!cfg) return;

                const replicaPoint = [cfg.lat, cfg.lng] as [number, number];

                L.polyline([leaderPoint, replicaPoint], {
                  color: "#1976d2",
                  weight: 2,
                  opacity: 0.7,
                }).addTo(arrowsLayer);

                L.marker(replicaPoint, {
                  icon: L.divIcon({
                    className: "arrow-icon",
                    html: "➤",
                    iconSize: [20, 20],
                  }),
                }).addTo(arrowsLayer);
              });
          }
        }
      } catch (err) {
        console.error("💥 Error calling backend:", err);
      } finally {
        isFetching = false;
        const btn2 = document.getElementById(
          "refresh-map"
        ) as HTMLButtonElement | null;
        if (btn2) {
          btn2.disabled = false;
          btn2.innerText = "Actualizar mapa";
        }
      }
    }

    // Botón refresh (Leaflet control, pero lo vamos a centrar por CSS)
    const refreshBtn = new L.Control({ position: "topright" });
    refreshBtn.onAdd = () => {
      const div = L.DomUtil.create("div", "refresh-container");
      div.innerHTML = `
        <button id="refresh-map" class="refresh-button">
          Actualizar mapa
        </button>
      `;
      return div;
    };
    refreshBtn.addTo(map);

    setTimeout(() => {
      document
        .getElementById("refresh-map")
        ?.addEventListener("click", fetchRolesAndUpdate);
    }, 200);

    // Primer fetch
    fetchRolesAndUpdate();

    return () => {
      map.remove();
    };
  }, []);

  return <div id="map" />;
}

export default App;
