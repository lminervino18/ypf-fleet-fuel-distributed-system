// nodeConnections.ts
import net from "net";

const MSG_TYPE_HANDLER_FIRST = 0x69;
const MSG_TYPE_ROLE_QUERY = 0x07;
const MSG_TYPE_ROLE_RESPONSE = 0x08;

const HANDLER_IP = "127.0.0.1";
const HANDLER_PORT = 4000;

type PendingRequest = {
  resolve: (value: any) => void;
  reject: (err: any) => void;
  timeout: NodeJS.Timeout;
} | null;

class NodeConnection {
  private host: string;
  private port: number;
  private socket: net.Socket | null = null;
  private buffer: Buffer = Buffer.alloc(0);
  private pending: PendingRequest = null;

  // que sea opcional, no null | Promise
  private connecting?: Promise<void>;

  private closed = true;

  constructor(host: string, port: number) {
    this.host = host;
    this.port = port;
  }

  // Garantiza que tenemos un socket conectado y con handshake hecho
  private ensureConnected(): Promise<void> {
    if (this.socket && !this.closed) {
      return Promise.resolve();
    }

    if (this.connecting) {
      // acá TS ya sabe que es Promise<void>
      return this.connecting;
    }

    // usamos una variable local tipada Promise<void>
    const p = new Promise<void>((resolve, reject) => {
      const socket = new net.Socket();
      this.socket = socket;
      this.closed = false;

      socket.on("connect", () => {
        // ==============================
        // HANDSHAKE: HANDLER_FIRST (0x69)
        // payload: [0x69][IP0][IP1][IP2][IP3][PORT_HI][PORT_LO]
        // ==============================
        const body = Buffer.alloc(7);
        body.writeUInt8(MSG_TYPE_HANDLER_FIRST, 0);

        const [a, b, c, d] = HANDLER_IP.split(".").map((x) => parseInt(x, 10));
        body.writeUInt8(a || 127, 1);
        body.writeUInt8(b || 0, 2);
        body.writeUInt8(c || 0, 3);
        body.writeUInt8(d || 1, 4);
        body.writeUInt16BE(HANDLER_PORT, 5);

        const frame = Buffer.alloc(2 + body.length);
        frame.writeUInt16BE(body.length, 0);
        body.copy(frame, 2);

        socket.write(frame, () => {
          // Handshake enviado, ya podemos usar el socket
          resolve();
        });
      });

      socket.on("data", (chunk) => this.onData(chunk));

      socket.on("error", (err) => {
        console.error(
          `[NODE-CONN] Socket error to ${this.host}:${this.port}`,
          err
        );
        this.handleSocketCloseOrError(err);
        reject(err);
      });

      socket.on("close", () => {
        console.warn(
          `[NODE-CONN] Socket closed for ${this.host}:${this.port}`
        );
        this.handleSocketCloseOrError(new Error("socket closed"));
        // no llamamos reject acá porque ya puede haberse manejado en error/timeout
      });

      socket.connect(this.port, this.host);
    });

    this.connecting = p;

    p.finally(() => {
      // cuando termina el intento de conexión, limpiamos el flag
      this.connecting = undefined;
    });

    return p;
  }

  private handleSocketCloseOrError(err: any) {
    this.closed = true;

    if (this.pending) {
      clearTimeout(this.pending.timeout);
      this.pending.reject(err);
      this.pending = null;
    }

    if (this.socket) {
      this.socket.destroy();
      this.socket = null;
    }
  }

  private onData(chunk: Buffer) {
    this.buffer = Buffer.concat([this.buffer, chunk]);

    while (this.buffer.length >= 2) {
      const len = this.buffer.readUInt16BE(0);
      if (this.buffer.length < 2 + len) break;

      const payload = this.buffer.slice(2, 2 + len);
      this.buffer = this.buffer.slice(2 + len);

      const msgType = payload.readUInt8(0);

      if (msgType === MSG_TYPE_HANDLER_FIRST) {
        // payload: [0x69][IP0][IP1][IP2][IP3][PORT_HI][PORT_LO]
        const ip0 = payload.readUInt8(1);
        const ip1 = payload.readUInt8(2);
        const ip2 = payload.readUInt8(3);
        const ip3 = payload.readUInt8(4);
        const port = payload.readUInt16BE(5);
        console.log(
          `[NODE-CONN] HANDLER_FIRST recibido desde nodo ${ip0}.${ip1}.${ip2}.${ip3}:${port}`
        );
      } else if (msgType === MSG_TYPE_ROLE_RESPONSE) {
        // payload: [0x08][node_id u64][role u8]
        if (!this.pending) {
          console.warn(
            "[NODE-CONN] ROLE_RESPONSE recibido sin pending activo"
          );
          continue;
        }

        const nodeId = payload.readBigUInt64BE(1);
        const role = payload.readUInt8(9);

        clearTimeout(this.pending.timeout);
        this.pending.resolve({
          ok: true,
          nodeId: nodeId.toString(),
          role,
        });
        this.pending = null;
      } else {
        console.log(
          `[NODE-CONN] Mensaje ignorado type=${msgType} len=${len} desde ${this.host}:${this.port}`
        );
      }
    }
  }

  // Envia un ROLE_QUERY y espera el ROLE_RESPONSE
  public async queryRole(): Promise<any> {
    await this.ensureConnected();

    if (!this.socket || this.closed) {
      throw new Error("Socket not connected");
    }

    if (this.pending) {
      // Para simplificar: no permitimos más de una query simultánea por nodo
      throw new Error("Another pending ROLE_QUERY for this node");
    }

    return new Promise<any>((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.pending = null;
        reject(new Error("timeout waiting ROLE_RESPONSE"));
      }, 2000);

      this.pending = { resolve, reject, timeout };

      // ROLE_QUERY (0x07)
      // payload: [0x07][IP0][IP1][IP2][IP3][PORT_HI][PORT_LO]
      const body = Buffer.alloc(7);
      body.writeUInt8(MSG_TYPE_ROLE_QUERY, 0);

      const [a, b, c, d] = HANDLER_IP.split(".").map((x) => parseInt(x, 10));
      body.writeUInt8(a || 127, 1);
      body.writeUInt8(b || 0, 2);
      body.writeUInt8(c || 0, 3);
      body.writeUInt8(d || 1, 4);
      body.writeUInt16BE(HANDLER_PORT, 5);

      const frame = Buffer.alloc(2 + body.length);
      frame.writeUInt16BE(body.length, 0);
      body.copy(frame, 2);

      this.socket!.write(frame);
    });
  }
}

// ==============================
// Manager global
// ==============================

const connections = new Map<string, NodeConnection>();

export function getNodeConnection(host: string, port: number): NodeConnection {
  const key = `${host}:${port}`;
  let conn = connections.get(key);
  if (!conn) {
    conn = new NodeConnection(host, port);
    connections.set(key, conn);
  }
  return conn;
}

// ⬇️ SOLO CAMBIAMOS ESTO: atrapamos errores y devolvemos "disconnected"
export async function queryNodeRole(
  host: string,
  port: number
): Promise<any> {
  const conn = getNodeConnection(host, port);

  try {
    const res = await conn.queryRole();
    return res;
  } catch (err: any) {
    console.warn(
      `[NODE-CONN] queryNodeRole failed for ${host}:${port}:`,
      err?.message ?? err
    );

    return {
      ok: false,
      role: "disconnected",
      error: err?.message ?? "connection error",
    };
  }
}
