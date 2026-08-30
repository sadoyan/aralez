job "echo" {
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
  group "Echo" {
    network {
      port "echo" { to = 80 }
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
        port = "echo"
        tags = [
          "aralez.service=yes",
          "aralez.host=echo.blablabla.com",
          "aralez.path=/",
          "aralez.rate=100",
          "aralez.4xx_rate=50",
          "aralez.to_https=false",
          "aralez.client_headers=WyJYLUVjaG8tU2VuZDpIb29vb29vb29vb29vIiwiWC1FY2hvLVJlY3Y6WW9vb29vb29vb29vbyJdCg==",
          "aralez.server_headers=WyJYLVByb3h5LUZyb206IEFyYWxleiIsIlgtQmFyaS1MdXlzOiBBcGVyIEphbiIsIlgtQW5rYXAtSGVhZGVyOiBCZXNhbXAgQW5rYXAgSGVhZGVyIl0K"
        ]
        check {
          type     = "http"
          port     = "echo"
          path     = "/"
          interval = "5s"
          timeout  = "2s"
        }
      }

      driver = "docker"

      config {
        image      = "ealen/echo-server:latest"
        force_pull = true
        ports = ["echo"]
      }
    }
  }
}