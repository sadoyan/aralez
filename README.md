![Gazan](https://netangels.net/utils/gazan-black.jpg)

# Gazan - The beast-mode reverse proxy.

Is a Reverse proxy, service mesh based on Cloudflare's Pingora

**Why Gazan ?** Roots and meaning (Gazan = Գազան = beast / wild animal in Armenian).

Built on Rust, on top of **Cloudflare’s battle-tested Pingora engine**, **Gazan** delivers world-class performance, security, and scalability — right out of the box.

**Pingora** powers millions of requests per second at Cloudflare’s edge, and now you can harness its core in your own infrastructure. This project brings that power into a lean and flexible reverse proxy with dynamic upstream configuration and
automatic websocket support.

---

## 🌍 Highlights

- ⚙️ **Upstream Providers:** Supports `file`-based static upstreams, dynamic service discovery via `Consul`, and upcoming `Kubernetes` integration
- 🔁 **Hot Reloading:** Modify upstreams on the fly via `upstreams.yaml` — no restart needed
- 🔮 **Automatic WebSocket Support:** No special config required — connection upgrades are handled seamlessly
- 🔐 **TLS Termination:** Fully supports TLS for incoming and upstream traffic
- 🛡️ **Built-in Auth Support:** (Basic and API Key ready)
- 🧠 **CORS & Header Injection:** Global and per-route header configuration
- 🧪 **Health Checks:** Pluggable health check methods for upstreams
- 🛰️ **Remote Config Push:** Lightweight HTTP API to update configs from CI/CD or other systems

---

## 📁 File Structure

```
.
├── main.yaml           # Main configuration loaded at startup
├── upstreams.yaml      # Watched config with upstream mappings
├── etc/
│   ├── server.crt      # TLS certificate (required if using TLS)
│   └── key.pem         # TLS private key
```

---

## 🛠 Configuration Overview

### 🔧 `main.yaml`

- `proxy_address_http`: `0.0.0.0:6193` (HTTP listener)
- `proxy_address_tls`: `0.0.0.0:6194` (TLS listener, optional)
- `config_address`: `0.0.0.0:3000` (HTTP API for remote config push)
- `upstreams_conf`: `etc/upstreams.yaml` (location of upstreams config)
- `log_level`: `info` (verbosity of logs)
- `hc_method`: `HEAD`, `hc_interval`: `2s` (upstream health checks)
- Other defaults: thread count, keep-alive pool size, etc.

### 🌐 `upstreams.yaml`

- `provider`: `file` or `consul`
- File-based upstreams define:
    - Hostnames and routing paths
    - Backend servers (load-balanced)
    - Optional request headers
    - Optional TLS for upstreams
- Global headers (e.g., CORS) apply to all proxied responses
- Optional authentication (Basic, API Key) — currently commented for example

---

## 🔌 Running the Proxy

```bash
./gazan -c path/to/main.yaml
```

Replace `APP_BINARY` with your compiled binary.

---

## 💡 Example

A sample `upstreams.yaml` entry:

```yaml
myhost.mydomain.com:
  paths:
    "/":
      ssl: false
      headers:
        - "X-Some-Thing:Yaaaaaaaaaaaaaaa"
        - "X-Proxy-From:Hopaaaaaaaaaaaar"
      servers:
        - "127.0.0.1:8000"
        - "127.0.0.2:8000"
```

This means:

- Requests to `myhost.mydomain.com/` will be load balanced to those servers.
- You can choose any path, deep nested paths are supported, the best match will be chosen
- Additional headers will be injected into the request.
- TLS is disabled for upstreams (but can be enabled).

---

## 🔄 Hot Reload

- Changes to `upstreams.yaml` are applied immediately.
- No need to restart the proxy — just save the file.

---

## 🔐 TLS Support

To enable TLS for Proxy server: Currently only OpenSSL is supported, working on Boringssl and Rustls

1. Set `proxy_address_tls` in `main.yaml`
2. Provide `tls_certificate` and `tls_key_file`

---

## 📡 Remote Config API

You can push new `upstreams.yaml` over HTTP to `config_address` (`:3000` by default). Useful for CI/CD automation or remote config updates.

```bash
curl -XPOST --data-binary @./etc/upstreams.txt 127.0.0.1:3000/conf
```

---

## 📃 License

The product is distributed under [Apache License Version 2.0](https://www.apache.org/licenses/LICENSE-2.0)

---

## 🧠 Notes

- Uses Pingora under the hood for efficiency and flexibility.
- Designed for edge proxying, internal routing, or hybrid cloud scenarios.
- Transparent, fully automatic WebSocket upgrade support. 