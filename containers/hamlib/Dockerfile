FROM debian:bookworm-slim

ARG HAMLIB_VERSION=4.7.2
ARG HAMLIB_SHA256=ae1fcf2dbc80ea0786ea8f047b09399c3f7737d1930442f61a031708ed33e88f

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl build-essential pkg-config libusb-1.0-0-dev \
      libudev-dev libreadline-dev libncurses-dev libxml2-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /tmp/hamlib
RUN curl --fail --location --silent --show-error \
      "https://github.com/Hamlib/Hamlib/releases/download/${HAMLIB_VERSION}/hamlib-${HAMLIB_VERSION}.tar.gz" \
      --output hamlib.tar.gz \
    && echo "${HAMLIB_SHA256}  hamlib.tar.gz" | sha256sum --check - \
    && tar -xzf hamlib.tar.gz --strip-components=1 \
    && ./configure --prefix=/usr --disable-static \
    && make -j"$(nproc)" \
    && make install \
    && ldconfig \
    && rm -rf /tmp/hamlib

ENV RIGCTLD_MODEL=1
ENV RIGCTLD_DEVICE=/dev/ttyUSB0
ENV RIGCTLD_PORT=4532

EXPOSE 4532
ENTRYPOINT ["/bin/sh", "-c", "exec rigctld -m \"$RIGCTLD_MODEL\" -r \"$RIGCTLD_DEVICE\" -T 0.0.0.0 -t \"$RIGCTLD_PORT\""]
