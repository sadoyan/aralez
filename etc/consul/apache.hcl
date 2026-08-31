job "apache" {
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
  group "Apache" {
    network {
      port "apache" { to = 80 }
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
    task "health" {
      service {
        port = "apache"
        tags = [
          "aralez.service=yes",
          "aralez.host=apache.blablabla.com",
          "aralez.path=/",
          "aralez.redirect=http://checkip.amazonaws.com"
        ]
        check {
          type     = "http"
          port     = "apache"
          path     = "/"
          interval = "5s"
          timeout  = "2s"
        }
      }
      driver = "docker"
      config {
        image      = "httpd:latest"
        force_pull = true
        ports = ["apache"]
      }
    }
  }
}
