# syntax=docker/dockerfile:1.23.0@sha256:2780b5c3bab67f1f76c781860de469442999ed1a0d7992a5efdf2cffc0e3d769

FROM golang:alpine AS builder

# Update check: https://github.com/awslabs/aws-sigv4-proxy/releases
ARG AWS_SIGV4_PROXY_VERSION=1.11.1
# `go install pkg@version` refuses modules with `replace` directives in go.mod,
# so clone the tag and build from inside the module instead.
RUN apk add --no-cache git \
    && git clone --depth 1 --branch v${AWS_SIGV4_PROXY_VERSION} \
        https://github.com/awslabs/aws-sigv4-proxy.git /src \
    && cd /src \
    && go build -o /go/bin/aws-sigv4-proxy ./cmd/aws-sigv4-proxy

FROM alpine:3.22
COPY --from=builder /go/bin/aws-sigv4-proxy /usr/local/bin/
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
