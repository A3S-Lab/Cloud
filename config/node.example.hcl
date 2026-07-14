control_plane {
  url = "https://cloud.example.com"
  enrollment_token_env = "A3S_CLOUD_ENROLLMENT_TOKEN"
}

node {
  name = "worker-1"
  state_dir = "/var/lib/a3s-cloud-node"
  provider = "docker"
}
