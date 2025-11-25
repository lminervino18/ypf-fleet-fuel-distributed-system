import { useEffect } from "react";
import L from "leaflet";
import "leaflet/dist/leaflet.css";
import "./App.css";
import nodesConfig from "../../../nodes.json";
import ypfLogo from "./assets/ypf-logo.png"; // ajustá si tu estructura es distinta

// =====================
// TIPOS
// =====================
type BackendRole = "leader" | "replica" | "client" | "station" | "disconnected";
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
  role?: unknown; // puede venir número o string
  error?: string;
};

type RolesApiResponse = {
  ok: boolean;
  results: RoleQueryResult[];
};

const NODES: NodeConfig[] = nodesConfig as NodeConfig[];

// =====================
// ICONOS (mismo PNG, distinto color por CSS)
// =====================
const baseSize: [number, number] = [40, 40];
const baseAnchor: [number, number] = [20, 40];

const iconLeader = L.icon({
  iconUrl: ypfLogo,
  iconSize: baseSize,
  iconAnchor: baseAnchor,
  className: "ypf-marker ypf-marker--leader",
});

const iconReplica = L.icon({
  iconUrl: ypfLogo,
  iconSize: baseSize,
  iconAnchor: baseAnchor,
  className: "ypf-marker ypf-marker--replica",
});

const iconClient = L.icon({
  iconUrl: ypfLogo,
  iconSize: baseSize,
  iconAnchor: baseAnchor,
  className: "ypf-marker ypf-marker--client",
});

const iconDisconnected = L.icon({
  iconUrl: ypfLogo,
  iconSize: baseSize,
  iconAnchor: baseAnchor,
  className: "ypf-marker ypf-marker--disconnected",
});

const iconUnknown = L.icon({
  iconUrl: ypfLogo,
  iconSize: baseSize,
  iconAnchor: baseAnchor,
  className: "ypf-marker ypf-marker--unknown",
});

// =====================
// Helpers
// =====================
function normalizeRole(role: any): NodeRole {
  // Leader
  if (role === 0 || role === "0" || role === "leader" || role === "Leader") {
    return "leader";
  }

  // Replica
  if (role === 1 || role === "1" || role === "replica" || role === "Replica") {
    return "replica";
  }

  // Client / station (aceptamos 2, 3 y strings)
  if (
    role === 2 ||
    role === "2" ||
    role === 3 ||
    role === "3" ||
    role === "client" ||
    role === "Client" ||
    role === "station"
  ) {
    return "client";
  }

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

      // Hover: agrandar manteniendo clases
      marker.on("mouseover", () => {
        const base = baseIconByKey[key];
        const size = base.options.iconSize as [number, number];
        const anchor = base.options.iconAnchor as [number, number];
        const className = base.options.className as string | undefined;

        marker.setIcon(
          L.icon({
            iconUrl: base.options.iconUrl!,
            iconSize: [size[0] * 1.4, size[1] * 1.4],
            iconAnchor: [anchor[0] * 1.4, anchor[1] * 1.4],
            className,
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

        const json = (await res.json()) as RolesApiResponse;
        console.log("📦 Parsed /api/roles JSON:", json);

        if (!json.ok) {
          console.error("❌ Backend returned ok=false", json);
          return;
        }

        arrowsLayer.clearLayers();

        // =====================
        // 1) Resolver líder "canónico"
        // =====================
        const leaders = json.results.filter(
          (r) => r.ok && normalizeRole(r.role) === "leader"
        );
        const canonicalLeader = leaders[0]; // si hay >1, nos quedamos con el primero
        const canonicalLeaderKey = canonicalLeader
          ? keyForNode(canonicalLeader)
          : null;

        // =====================
        // 2) Actualizar marcadores (forzando un solo líder)
        // =====================
        json.results.forEach((result) => {
          const key = keyForNode(result);
          const marker = markersByKey[key];
          const cfg = nodeByKey[key];
          if (!marker || !cfg) return;

          let normalized: NodeRole;

          if (!result.ok) {
            normalized = "disconnected";
          } else {
            const rawRole = normalizeRole(result.role);

            if (rawRole === "leader") {
              // Si este es el líder canónico → leader
              // Si es otro que también dice líder → lo degradamos visualmente a replica
              if (canonicalLeaderKey && key === canonicalLeaderKey) {
                normalized = "leader";
              } else {
                normalized = "replica"; // o "unknown", como prefieras
              }
            } else {
              normalized = rawRole;
            }
          }

          const nodeIdText = result.nodeId ?? "-";
          const icon = iconForRole(normalized);
          marker.setIcon(icon);
          baseIconByKey[key] = icon;

          const name = cfg.name ?? key;
          const estado = result.ok ? "ONLINE" : "DISCONNECTED";

          marker.bindPopup(`
            <b>${name}</b><br>
            ID: <b>${nodeIdText}</b><br>
            ROL: <b>${normalized.toUpperCase()}</b><br>
            ESTADO: ${estado}${
            result.error
              ? `<br><small style="color:red">${result.error}</small>`
              : ""
          }
          `);
        });

        // =====================
        // 3) Dibujar flechas leader → replicas
        // =====================
        if (canonicalLeader && canonicalLeaderKey) {
          const leaderCfg = nodeByKey[canonicalLeaderKey];
          if (leaderCfg) {
            const leaderPoint: [number, number] = [
              leaderCfg.lat,
              leaderCfg.lng,
            ];

            json.results
              .filter(
                (r) => r.ok && normalizeRole(r.role) === "replica"
              )
              .forEach((rep) => {
                const cfg = nodeByKey[keyForNode(rep)];
                if (!cfg) return;

                const replicaPoint: [number, number] = [
                  cfg.lat,
                  cfg.lng,
                ];

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

    // Botón refresh
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

    // Primer fetch automático
    fetchRolesAndUpdate();

    return () => {
      map.remove();
    };
  }, []);

  return <div id="map" />;
}

export default App;
