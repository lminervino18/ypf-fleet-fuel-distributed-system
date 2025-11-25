// server.ts
import express from "express";
import cors from "cors";
import { queryNodeRole } from "./nodeConnections";

const HTTP_PORT = 3988;

// ==========================
// Manejo global de errores
// ==========================
process.on("uncaughtException", (err) => {
  console.error("[FATAL] Uncaught exception en backend HTTP:", err);
  // NO hacemos process.exit, dejamos vivo el proceso en dev
});

process.on("unhandledRejection", (reason, promise) => {
  console.error(
    "[FATAL] Unhandled rejection en backend HTTP:",
    reason,
    "en",
    promise
  );
  // Tampoco salimos en dev
});

// ==========================
// App Express
// ==========================
const app = express();

// En dev, abrimos a cualquier puerto de localhost
app.use(
  cors({
    origin: (origin, callback) => {
      if (!origin) return callback(null, true); // curl / same-origin
      if (origin.startsWith("http://localhost:")) {
        return callback(null, true);
      }
      callback(new Error("Not allowed by CORS"));
    },
  })
);

app.use(express.json());

// Endpoint simple de healthcheck (opcional, pero útil para debug)
app.get("/health", (_req, res) => {
  res.json({ ok: true, message: "backend up" });
});

// ==========================
// /api/roles
// ==========================
app.post("/api/roles", async (req, res) => {
  const nodes: { host: string; port: number }[] = req.body.nodes ?? [];
  const results: any[] = [];

  // Importante: queryNodeRole YA atrapa errores internos
  // y devuelve { ok:false, role:"disconnected", error:... }
  for (const n of nodes) {
    try {
      const r = await queryNodeRole(n.host, n.port);

      results.push({
        host: n.host,
        port: n.port,
        ...r,
      });
    } catch (err: any) {
      // Este catch es "cinturón y tirantes", por si algo raro se escapa
      console.error("[HTTP] Error inesperado querying node", n, err);
      results.push({
        host: n.host,
        port: n.port,
        ok: false,
        role: "disconnected",
        error: err?.message ?? "exception in backend",
      });
    }
  }

  res.json({ ok: true, results });
});

// ==========================
// Arranque del servidor
// ==========================
app.listen(HTTP_PORT, () => {
  console.log(`HTTP backend listening on http://localhost:${HTTP_PORT}`);
});
