import express from "express";
import cors from "cors";
import { RustTcpClient } from "./tcpClient";
import { startTcpServer } from "./tcpServer";

const HTTP_PORT = 3001;

startTcpServer();

const app = express();
app.use(cors({ origin: "http://localhost:5173" }));
app.use(express.json());

app.post("/api/roles", async (req, res) => {
  const nodes: { host: string; port: number }[] = req.body.nodes ?? [];

  const results: any[] = [];

  // 🔁 Consulta secuencial: un nodo por vez, esperando respuesta
  for (const n of nodes) {
    try {
      const r = await new RustTcpClient(n.host, n.port).queryRole();
      // Devolvemos también host/port para que el front pueda mapear el nodo
      results.push({
        host: n.host,
        port: n.port,
        ...r,
      });
    } catch (err) {
      console.error("[HTTP] Error querying node", n, err);
      results.push({
        host: n.host,
        port: n.port,
        ok: false,
        role: "disconnected",
        error: "exception in backend",
      });
    }
  }

  res.json({ ok: true, results });
});

app.listen(HTTP_PORT, () => {
  console.log(`HTTP backend listening on http://localhost:${HTTP_PORT}`);
});
