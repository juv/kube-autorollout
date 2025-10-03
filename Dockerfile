FROM rust:1.89-alpine as builder

WORKDIR /usr/src/app
COPY . .

# Install the target for static linking (musl for Alpine/scratch)
RUN rustup target add x86_64-unknown-linux-musl

RUN apk add musl-dev openssl-dev openssl-libs-static

# Build the application in release mode with static linking, compressing debug sections
ENV RUSTFLAGS="-C link-arg=-Wl,--compress-debug-sections=zlib"
RUN cargo build --release --target x86_64-unknown-linux-musl

FROM scratch

COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-musl/release/kube-autorollout /kube-autorollout
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

USER 1001

ENTRYPOINT ["/kube-autorollout"]