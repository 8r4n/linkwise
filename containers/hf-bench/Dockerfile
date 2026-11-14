FROM ubuntu:24.04
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends gnuradio=3.10.9.2-1.1ubuntu2 \
    && rm -rf /var/lib/apt/lists/*
