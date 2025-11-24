import net from "net";

export const TCP_SERVER_PORT = 4000;

type PendingResponse =
  | {
      resolve: (value: any) => void;
      timeout: NodeJS.Timeout;
    }
  | null;

// Estado global: solo una respuesta pendiente a la vez
export const responseState: { pending: PendingResponse } = {
  pending: null,
};

export function startTcpServer() {
  const server = net.createServer((socket) => {
    console.log(
      "[TCP-SERVER] New connection from:",
      socket.remoteAddress,
      socket.remotePort
    );

    let buffer = Buffer.alloc(0);

    socket.on("data", (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);

      while (buffer.length >= 2) {
        const len = buffer.readUInt16BE(0);
        if (buffer.length < 2 + len) break;

        const payload = buffer.slice(2, 2 + len);
        buffer = buffer.slice(2 + len);

        const msgType = payload.readUInt8(0);

        if (msgType === 0x08) {
          const nodeId = payload.readBigUInt64BE(1);
          const role = payload.readUInt8(9);

          console.log(
            `[BACKEND] ROLE_RESPONSE id=${nodeId}, role=${role}, from=${socket.remoteAddress}:${socket.remotePort}`
          );

          const pending = responseState.pending;

          if (pending) {
            clearTimeout(pending.timeout);

            pending.resolve({
              ok: true,
              nodeId: nodeId.toString(),
              role,
            });

            responseState.pending = null;
          } else {
            console.warn(
              "[BACKEND] ROLE_RESPONSE recibido pero no hay pending activa"
            );
          }
        }
      }
    });
  });

  server.listen(TCP_SERVER_PORT, () => {
    console.log(
      `[TCP-SERVER] Listening for node responses on port ${TCP_SERVER_PORT}`
    );
  });
}
