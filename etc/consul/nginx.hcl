job "nginx" {
  meta {
    uuid = uuidv4()
  }
  datacenters = ["eu-ams-1"]
  type = "service"
  constraint {
    attribute = node.unique.name
    operator  = "regexp"
    value     = "consul[0-9]"
  }
  update {
    max_parallel      = 1
    min_healthy_time  = "10s"
    healthy_deadline  = "3m"
    progress_deadline = "10m"
    auto_revert       = false
    canary            = 0
  }
  migrate {
    max_parallel     = 1
    health_check     = "checks"
    min_healthy_time = "10s"
    healthy_deadline = "5m"
  }
  group "NginX" {
    network {
      port "nginx" { to = 80 }
    }
    constraint {
      operator = "distinct_hosts"
      value    = "true"
    }
    count = 3
    restart {
      attempts = 2
      interval = "30m"
      delay    = "15s"
      mode     = "fail"
    }
    task "server" {
      service {
        port = "nginx"
        tags = [
          "aralez.service=yes",
          "aralez.host=nginx.blablabla.com",
          "aralez.path=/",
          "aralez.rate=20",
          "aralez.4xx_rate=10",
          "aralez.to_https=true",
          "aralez.auth=basic root:toor",
          "aralez.client_headers=WwogICAgIlgtU29tZS1IZWFkZXI6QUFBQUFBQUFBQUFBQUFBIiwKICAgICJYLUVtb3MtSGVhZGVyOlpaWlpaWlpaWlpaWlpaWiIsCiAgICAiWC1PdGhlci1oZWFkZXI6VmF5VmF5VmF5IiwKICAgICJYLVJlaHRvLWhlYWRlcjpaYXlaYXlaYXkiCl0KCg==",
          "aralez.server_headers=WyJYLVByb3h5LUZyb206IEFyYWxleiIsIlgtQmFyaS1MdXlzOiBBcGVyIEphbiIsIlgtQW5rYXAtSGVhZGVyOiBCZXNhbXAgQW5rYXAgSGVhZGVyIl0K"
        ]
        check {
          type     = "http"
          port     = "nginx"
          path     = "/"
          interval = "5s"
          timeout  = "2s"
        }
      }

      driver = "docker"

      config {
        image      = "nginx:latest"
        force_pull = true
        ports = ["nginx"]
      }
    }
  }
}