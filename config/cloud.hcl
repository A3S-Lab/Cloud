server {
  host = "127.0.0.1"
  port = 8080
  role = "all"
}

postgres {
  url_env = "A3S_CLOUD_POSTGRES_URL"
  max_connections = 16
}

auth {
  bootstrap_token_env = "A3S_CLOUD_BOOTSTRAP_TOKEN"
}

events {
  provider = "memory"
  nats_url_env = "A3S_CLOUD_NATS_URL"
  stream_name = "A3S_CLOUD_EVENTS"
  batch_size = 100
  poll_interval_ms = 250
  lease_ms = 10000
  publish_timeout_ms = 3000
  retry_initial_ms = 500
  retry_max_ms = 30000
}

operations {
  reconcile_interval_ms = 5000
  lease_ms = 30000
}
