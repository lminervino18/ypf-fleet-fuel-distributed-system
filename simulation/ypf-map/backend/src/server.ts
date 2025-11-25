// server.ts
import express from "express";
import cors from "cors";
import { queryNodeRole } from "./nodeConnections";

const HTTP_PORT = 3988; // poné acá el que realmente te está usando ts-node-dev

const app = express();

// En dev, abrimos a cualquier puerto de localhost
app.use(
  cors({
    origin: (origin, callback) => {
      if (!origin) return callback(null, true); // requests tipo curl / same-origin
      if (origin.startsWith("http://localhost:")) {
        return callback(null, true);
      }
      callback(new Error("Not allowed by CORS"));
    },
  })
);

app.use(express.json());

app.post("/api/roles", async (req, res) => {
  const nodes: { host: string; port: number }[] = req.body.nodes ?? [];
  const results: any[] = [];

  for (const n of nodes) {
    try {
      const r = await queryNodeRole(n.host, n.port);
      results.push({
        host: n.host,
        port: n.port,
        ...r,
      });
    } catch (err: any) {
      console.error("[HTTP] Error querying node", n, err);
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

app.listen(HTTP_PORT, () => {
  console.log(`HTTP backend listening on http://localhost:${HTTP_PORT}`);
});
