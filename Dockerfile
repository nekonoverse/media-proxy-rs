FROM --platform=$BUILDPLATFORM public.ecr.aws/docker/library/rust:latest AS cross_build
ARG BUILDARCH
ARG TARGETARCH
ARG TARGETVARIANT
RUN apt-get update && apt-get install -y clang musl-dev pkg-config nasm mold git meson ninja-build xz-utils
COPY crossfiles /app/crossfiles
RUN bash /app/crossfiles/deps.sh

FROM --platform=$BUILDPLATFORM cross_build AS dav1d
RUN git clone --branch 1.4.3 --depth 1 https://github.com/videolan/dav1d.git /dav1d_src
RUN cd /dav1d_src && bash -c "source /app/crossfiles/meson.sh && meson build -Dprefix=/dav1d -Denable_tools=false -Denable_examples=false -Ddefault_library=static --buildtype release --cross-file /app/crossfiles/cross.txt"
RUN cd /dav1d_src && bash -c "source /app/crossfiles/meson.sh && ninja -C build"
RUN cd /dav1d_src && bash -c "source /app/crossfiles/meson.sh && ninja -C build install"

FROM --platform=$BUILDPLATFORM cross_build AS lcms2
RUN git clone -b lcms2.16 --depth 1 https://github.com/mm2/Little-CMS.git /lcms2_src
RUN mkdir /lcms2
RUN cd /lcms2_src && bash -c "source /app/crossfiles/meson.sh && meson build --prefix=/lcms2 -Ddefault_library=static -Dfastfloat=true -Dthreaded=true --buildtype release --cross-file /app/crossfiles/cross.txt"
RUN cd /lcms2_src && bash -c "source /app/crossfiles/meson.sh && ninja -C build"
RUN cd /lcms2/ && cp /lcms2_src/build/src/liblcms2.a . && cp /lcms2_src/build/plugins/threaded/src/liblcms2_threaded.a . && cp /lcms2_src/build/plugins/fast_float/src/liblcms2_fast_float.a .

FROM --platform=$BUILDPLATFORM cross_build AS build_app
ENV CARGO_HOME=/var/cache/cargo
ENV SYSTEM_DEPS_LINK=static
WORKDIR /app
COPY avif-decoder_dep ./avif-decoder_dep
COPY .gitmodules ./.gitmodules
COPY --from=dav1d /dav1d /dav1d
COPY --from=lcms2 /lcms2 /lcms2
RUN cp -r /lcms2/* /dav1d/lib
ENV PKG_CONFIG_PATH=/dav1d/lib/pkgconfig
ENV LD_LIBRARY_PATH=/dav1d/lib
COPY src ./src
COPY Cargo.toml ./Cargo.toml
COPY asset ./asset
COPY examples ./examples
RUN --mount=type=cache,target=/var/cache/cargo --mount=type=cache,target=/app/target bash /app/crossfiles/build.sh

# smoke test stage: runtime が distroless (shell なし) なのでビルド時自己テストは別ステージで実行する。
# プラットフォーム指定なし = TARGETPLATFORM、buildx + QEMU 経由で cross-emulation される。
FROM public.ecr.aws/docker/library/alpine:latest AS smoke_test
WORKDIR /test
COPY --from=build_app /app/media-proxy-rs ./media-proxy-rs
COPY --from=build_app /app/healthcheck ./healthcheck
RUN sh -c "MEDIA_PROXY_ALLOWED_NETWORKS=127.0.0.0/8 ./media-proxy-rs&" && ./healthcheck 12887 http://127.0.0.1:12766/test.webp && touch /test/passed

# runtime stage 用の rootfs を 852 所有でステージングする (distroless には mkdir/chown が無く、
# COPY の --chown も dest dir auto-create には効かないため、build stage で完全な階層構造を組む)。
# original alpine 版で adduser -h /media-proxy-rs が果たしていた役割と同じ。
FROM --platform=$BUILDPLATFORM cross_build AS runtime_home
RUN mkdir -p /rootfs/media-proxy-rs
COPY --from=build_app /app/media-proxy-rs /rootfs/media-proxy-rs/media-proxy-rs
COPY --from=build_app /app/healthcheck /rootfs/media-proxy-rs/healthcheck
RUN chown -R 852:852 /rootfs/media-proxy-rs && chmod 755 /rootfs/media-proxy-rs/media-proxy-rs /rootfs/media-proxy-rs/healthcheck

# runtime: gcr.io/distroless/static-debian13 (CA certs + tzdata + libc-stub のみ、約 2MB)
FROM gcr.io/distroless/static-debian13:latest
# rootfs まるごと / に展開すると /media-proxy-rs ディレクトリの ownership (852:852) も保持される
COPY --from=runtime_home /rootfs/ /
# smoke test stage の成果物を取り込むことでテスト失敗時にビルドを失敗させる
COPY --from=smoke_test /test/passed /etc/smoke-passed
WORKDIR /media-proxy-rs
USER 852:852
HEALTHCHECK --interval=30s --timeout=3s CMD ["./healthcheck", "5555", "http://127.0.0.1:12766/test.webp"]
EXPOSE 12766
CMD ["./media-proxy-rs"]
