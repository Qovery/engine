FROM ghcr.io/opentofu/opentofu:{{provider_version}}-minimal AS opentofu

FROM alpine:3.22

COPY --from=opentofu /usr/local/bin/tofu /usr/local/bin/tofu

RUN <<EOF
set -e
apk update
apk add dumb-init rsync bash
adduser -D -u 1000 app
mkdir /data
chown -R app:app /data
EOF


WORKDIR /data
COPY --chown=app:app . .

RUN chmod +x entrypoint.sh
USER app

ENTRYPOINT ["/usr/bin/dumb-init", "--", "/bin/sh", "/data/entrypoint.sh"]
