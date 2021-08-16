FROM gitpod/workspace-full

RUN sudo apt-get update \
 && apt-get install -y tree \
 && rm -rf /var/lib/apt/lists/*
