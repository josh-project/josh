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

target "dev-ci" {
  context    = "."
  dockerfile = "images/dev-ci/Dockerfile"
  tags       = ["josh-ci-dev:latest"]
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
  context    = "."
  dockerfile = "images/run/Dockerfile"
  contexts = {
    josh-dev   = "target:dev"
    josh-build = "target:build"
  }
}
