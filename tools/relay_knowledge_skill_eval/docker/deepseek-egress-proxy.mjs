import net from "node:net";

const UPSTREAM_HOST = "api.deepseek.com";
const UPSTREAM_PORT = 443;
const LISTEN_PORT = 443;

const server = net.createServer((client) => {
  const upstream = net.connect({ host: UPSTREAM_HOST, port: UPSTREAM_PORT });
  client.pipe(upstream);
  upstream.pipe(client);

  const closeBoth = () => {
    client.destroy();
    upstream.destroy();
  };
  client.on("error", closeBoth);
  upstream.on("error", closeBoth);
  client.on("close", () => upstream.destroy());
  upstream.on("close", () => client.destroy());
});

server.listen(LISTEN_PORT, "0.0.0.0");
