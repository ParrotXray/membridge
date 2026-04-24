FROM ghcr.io/pyo3/maturin

USER root
RUN rustup target add x86_64-pc-windows-gnu \
    && pip install ziglang \
    && yum clean all