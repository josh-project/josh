# syntax = docker/dockerfile:1.4-labs

FROM alpine:3.16

RUN apk add --no-cache openssh bash git xz tree shadow

ARG S6_OVERLAY_VERSION=3.1.2.1
ADD https://github.com/just-containers/s6-overlay/releases/download/v${S6_OVERLAY_VERSION}/s6-overlay-noarch.tar.xz /tmp
RUN tar -C / -Jxpf /tmp/s6-overlay-noarch.tar.xz
ADD https://github.com/just-containers/s6-overlay/releases/download/v${S6_OVERLAY_VERSION}/s6-overlay-x86_64.tar.xz /tmp
RUN tar -C / -Jxpf /tmp/s6-overlay-x86_64.tar.xz

ARG GIT_GID_UID=2001

RUN addgroup -g ${GIT_GID_UID} git
RUN adduser \
    -h /home/git \
    -s josh-ssh-shell \
    -G git \
    -D \
    -u ${GIT_GID_UID} \
    git

# sshd will call josh-ssh-shell -c "git command"

# https://unix.stackexchange.com/a/193131/336647
RUN usermod -p '*' git

COPY josh-auth-key.sh /opt/scripts/
COPY etc/ssh/sshd_config /etc/ssh/

ARG RC6_D=/etc/s6-overlay/s6-rc.d

# s6: josh-generate-keys

COPY josh-generate-keys.sh /opt/scripts/

WORKDIR ${RC6_D}/josh-generate-keys

COPY <<EOF ./type
oneshot
EOF

COPY --chmod=755 <<EOF ./up
#!/command/execlineb -P
/opt/scripts/josh-generate-keys.sh
EOF

# s6: josh

WORKDIR ${RC6_D}/josh

COPY <<EOF ./dependencies
josh-generate-keys
EOF

COPY <<EOF ./type
longrun
EOF

COPY --chmod=755 <<EOF ./run
#!/command/execlineb -S0
loopwhilex -x 1
foreground
{
  sleep 1
}
EOF

COPY --chmod=755 ./s6-rc.d/finish ./

# s6: sshd

WORKDIR ${RC6_D}/sshd

COPY <<EOF ./type
longrun
EOF

COPY --chmod=755 <<EOF ./run
#!/command/execlineb -P
/usr/sbin/sshd -e -D -h /mnt/persistent/.ssh/id_ed25519
EOF

COPY --chmod=755 ./s6-rc.d/finish ./

# s6: service bundle

WORKDIR ${RC6_D}/user/contents.d

COPY <<EOF ./josh
EOF

COPY <<EOF ./sshd
EOF

WORKDIR /
ENV S6_BEHAVIOUR_IF_STAGE2_FAILS=2
ENTRYPOINT ["/init"]
