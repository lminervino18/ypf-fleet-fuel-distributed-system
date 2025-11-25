import net from "net";
import { responseState, TCP_SERVER_PORT } from "./tcpServer";

export class RustTcpClient {
  constructor(private host: string, private port: number) {}

  queryRole(): Promise<any> {
    return new Promise((resolve) => {
      // Si quedó alguna pending anterior colgada, la limpiamos
      if (responseState.pending) {
        clearTimeout(responseState.pending.timeout);
        responseState.pending = null;
      }

      // Registramos ESTA promesa como la única pendiente
      responseState.pending = {
        resolve,
        timeout: setTimeout(() => {
          resolve({
            ok: false,
            role: "disconnected",
            error: "timeout waiting ROLE_RESPONSE",
          });
          responseState.pending = null;
        }, 2000),
      };

      const socket = new net.Socket();

      socket.connect(this.port, this.host, () => {
        const body = Buffer.alloc(7);
        body.writeUInt8(0x07, 0); // TYPE=ROLE_QUERY
        body.writeUInt8(127, 1);
        body.writeUInt8(0, 2);
        body.writeUInt8(0, 3);
        body.writeUInt8(1, 4);
        body.writeUInt16BE(TCP_SERVER_PORT, 5); // puerto donde el backend espera el ROLE_RESPONSE

        const frame = Buffer.alloc(2 + body.length);
        frame.writeUInt16BE(body.length, 0);
        body.copy(frame, 2);

        socket.write(frame);
        socket.destroy(); // cerramos, la respuesta viene al TCP_SERVER_PORT
      });

      socket.on("error", () => {
        console.error(
          `[TCP-CLIENT] Connection error to ${this.host}:${this.port}`
        );
        resolve({
          ok: false,
          role: "disconnected",
          error: "connection error",
        });

        if (responseState.pending) {
          clearTimeout(responseState.pending.timeout);
          responseState.pending = null;
        }
      });
    });
  }
}
