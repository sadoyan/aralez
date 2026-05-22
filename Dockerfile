FROM debian:trixie-slim

RUN apt-get update && apt-get install -y ca-certificates curl net-tools iputils-ping
RUN apt-get clean && rm -rf /var/lib/apt/lists/*

COPY aralez /usr/local/bin/aralez

RUN chmod +x /usr/local/bin/aralez
RUN mkdir -p /etc/aralez/certs/upstreams

WORKDIR /etc/aralez

ENTRYPOINT ["/usr/local/bin/aralez", "-c", "/etc/aralez/main.yaml"]
