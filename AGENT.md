# Fork-specific constraints

This fork intentionally differs from upstream in the following deployment
choices. Preserve them unless the maintainer explicitly asks to change them.

## amd64 only

- Build, release, and end-to-end CI target **`linux/amd64` only**.
- Do not reintroduce QEMU, multi-platform Buildx matrices, or non-amd64
  crossfiles (`arm*`, `riscv*`, `386`).
- The Docker build scripts deliberately source `crossfiles/amd64.sh` directly.

## Distroless non-root runtime

- The final image must remain
  `gcr.io/distroless/static-debian13:nonroot`.
- It must run as the image's `nonroot` user (UID/GID 65532); do not add a
  shell, package manager, or root entrypoint to the runtime image.
- `/media-proxy-rs` is staged as UID/GID 65532 because the application creates
  `config.json` there on first startup.
- Runtime health checks use the static `healthcheck` executable. Any command
  requiring a shell belongs in the Alpine `smoke_test` build stage, never in
  the final image.
