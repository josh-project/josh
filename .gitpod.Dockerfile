FROM gitpod/workspace-full

RUN apt-get update \
 && apt-get install -y tree \
 && rm -rf /var/lib/apt/lists/*