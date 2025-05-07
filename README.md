![Gazan](https://netangels.net/utils/gazan-white.jpg)

# Gazan - The beast-mode reverse proxy.

Gazan is a Reverse proxy, service mesh based on Cloudflare's Pingora

**What Gazan means?**
<ins>Gazan = Գազան = beast / wild animal in Armenian / Often used as a synonym to something great.</ins>.

Built on Rust, on top of **Cloudflare’s Pingora engine**, **Gazan** delivers world-class performance, security, and scalability — right out of the box.

---

## 🌍 Highlights

- ⚙️ **Upstream Providers:** Supports `file`-based static upstreams, dynamic service discovery via `Consul`, and upcoming `Kubernetes` integration
- 🔁 **Hot Reloading:** Modify upstreams on the fly via `upstreams.yaml` — no restart needed
- 🔮 **Automatic WebSocket Support:** Zero config — connection upgrades are handled seamlessly
- 🔮 **Automatic GRPC Support:** Zero config, Requires `ssl` to proxy, gRPC is handled seamlessly
- 🔮 **Upstreams Session Stickiness:** Enable/Disable Sticky session support with single parameter in config file
- 🔐 **TLS Termination:** Fully supports TLS for incoming and upstream traffic
- 🛡️ **Built-in Auth Support:** Basic Auth, JWT, API key
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
- `user` Optional. Drop privileges to regular user. To bind to privileged ports. Requires to start as root.
- `group` Optional. Drop privileges to regular group
- Other defaults: thread count, keep-alive pool size, etc.

### 🌐 `upstreams.yaml`

- `provider`: `file` or `consul`
- File-based upstreams define:
    - Hostnames and routing paths
    - Backend servers (load-balanced)
    - Optional request headers, specific to this upstream
    - Optional TLS for upstreams
- Global headers (e.g., CORS) apply to all proxied responses
- Optional authentication (Basic, API Key, JWT)

---

## 🛠 Installation

Download the prebuilt binary for your architecture from releases section of [GitHub](https://github.com/sadoyan/gazan/releases) repo
Make the binary executable `chmod 755 ./gazan` and run .

## 🔌 Running the Proxy

```bash
./gazan -c path/to/main.yaml
```

## 🔌 Systemd integration

```bash
cat > /etc/systemd/system/gazan.service <<EOF
[Service]
Type=forking
PIDFile=/run/gazan.pid
ExecStart=/bin/gazan -d -c /etc/gazan.conf
ExecReload=kill -QUIT $MAINPID
ExecReload=/bin/gazan -u -d -c /etc/gazan.conf
EOF
```

```bash
systemctl enable gazan.service.
systemctl restart gazan.service.
```

## 💡 Example

A sample `upstreams.yaml` entry:

```yaml
provider: "file"
stickysessions: false
globals:
  headers:
    - "Access-Control-Allow-Origin:*"
    - "Access-Control-Allow-Methods:POST, GET, OPTIONS"
    - "Access-Control-Max-Age:86400"
  authorization:
    - "jwt"
    - "910517d9-f9a1-48de-8826-dbadacbd84af-cb6f830e-ab16-47ec-9d8f-0090de732774"
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
    "/foo":
      ssl: true
      headers:
        - "X-Another-Header:Hohohohoho"
      servers:
        - "127.0.0.4:8000"
        - "127.0.0.5:8000"
```

This means:

- Sticky sessions are disabled globally. This boolean setting applies to all upstreams.
- Requests to `myhost.mydomain.com/` will be load balanced to `127.0.0.1` and `127.0.0.2` servers via plain http.
- Requests to `myhost.mydomain.com/foo` will be load balanced to `127.0.0.4` and `127.0.0.5` servers via https.
- Global headers (CORS for this case) will be injected to all upstreams
- Additional headers will be injected into the request for `myhost.mydomain.com`.
- You can choose any path, deep nested paths are supported, the best match will be chosen
- TLS is disabled for upstreams (but can be enabled).
- All requests to servers will require JWT token authentication (You can comment out the authorization to disable it),
    - Firs parameter specifies the mechanism of authorisation `jwt`
    - Second is the secret key for validating `jwt` tokens

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

## 🔐 Authentication (Optional)

- Adds authentication to all requests.
- Only one method can be active at a time.
- `basic` : Standard HTTP Basic Authentication requests.
- `apikey` : Authentication via `x-api-key` header, which should match the value in config.
- `jwt`: JWT authentication implemented via `gazantoken=` url parameter. `/some/url?gazantoken=TOKEN`
- `jwt`: JWT authentication implemented via `Authorization: Bearer <token>` header.
    - To obtain JWT token, you should send **generate** request to built in api server's `/jwt` endpoint.
    - `masterkey`: should match configured `masterkey` in `main.yaml` and `upstreams.yaml`.
    - `owner` : Just a placeholder, can be anything.
    - `valid` : Time in minutes during which the generated token will be valid.

**Example JWT token generateion request**

```bash
PAYLOAD='{
    "masterkey": "910517d9-f9a1-48de-8826-dbadacbd84af-cb6f830e-ab16-47ec-9d8f-0090de732774",
    "owner": "valod",
    "valid": 10
}'

TOK=`curl -s -XPOST -H "Content-Type: application/json" -d "$PAYLOAD"  http://127.0.0.1:3000/jwt  | cut -d '"' -f4`
echo $TOK
```

**Example Request with JWT token**

With `Authorization: Bearer` header

```bash
curl -H "Authorization: Bearer ${TOK}" -H 'Host: myip.mydomain.com' http://127.0.0.1:6193/
```

With URL parameter (Very useful if you want to generate and share temporary links)

```bash
curl -H 'Host: myip.mydomain.com' "http://127.0.0.1:6193/?gazantoken=${TOK}`"
```

**Example Request with API Key**

```bash
curl -H "x-api-key: ${APIKEY}" --header 'Host: myip.mydomain.com' http://127.0.0.1:6193/

```

**Example Request with Basic Auth**

```bash
curl  -u username:password -H 'Host: myip.mydomain.com' http://127.0.0.1:6193/

```

## 📃 License

[Apache License Version 2.0](https://www.apache.org/licenses/LICENSE-2.0)

---

## 🧠 Notes

- Uses Pingora under the hood for efficiency and flexibility.
- Designed for edge proxying, internal routing, or hybrid cloud scenarios.
- Transparent, fully automatic WebSocket upgrade support.
- Transparent, fully automatic gRPC proxy.
- Sticky session support.
- HTTP2 ready.
- Upcoming Kubernetes integration