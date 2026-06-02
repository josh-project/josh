variable "REGISTRY" {
  default = "registry.internal.josh-project.dev"
}

variable "IMAGE_VERSION" {
  default = "v1"
}

group "default" {
  targets = ["josh-test-webhook-service"]
}

target "josh-test-webhook-service" {
  # Context is the repo root: the crate is a workspace member and the build
  # needs the whole workspace (Cargo.lock, sibling crates, etc.).
  context    = ".."
  dockerfile = "deployment/josh-test-webhook-service.Dockerfile"
  target     = "release"
  platforms  = ["linux/amd64"]
  tags       = ["${REGISTRY}/josh-test-webhook-service:${IMAGE_VERSION}"]
}
