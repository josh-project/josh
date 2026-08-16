target "rust-base" {
  context    = "."
  dockerfile = "images/rust-base/Dockerfile"
  tags       = ["josh-rust-base:latest"]
}

target "dev" {
  context    = "."
  dockerfile = "images/dev/Dockerfile"
  tags       = ["josh-dev:latest"]
  contexts = {
    josh-rust-base = "target:rust-base"
  }
}

target "dev-local" {
  context    = "."
  dockerfile = "images/dev-local/Dockerfile"
  tags       = ["josh-dev-local:latest"]
  contexts = {
    josh-dev = "target:dev"
  }
}

target "build" {
  context    = "."
  dockerfile = "images/build/Dockerfile"
  tags       = ["josh-build:latest"]
  contexts = {
    josh-dev = "target:dev"
  }
}

target "run" {
  context    = "images/run"
  dockerfile = "Dockerfile"
  contexts = {
    josh-dev   = "target:dev"
    josh-build = "target:build"
  }
}

target "josh-test-webhook-service" {
  # Context is the repo root: the crate is a workspace member and the build
  # needs the whole workspace (Cargo.lock, sibling crates, etc.).
  context    = "."
  dockerfile = "deployment/josh-test-webhook-service.Dockerfile"
  target     = "release"
  tags       = ["josh-test-webhook-service:latest"]
}
