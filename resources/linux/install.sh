#!/bin/sh
set -eu
cd -- "$(dirname -- "$0")"
if [ "$(id -u)" != 0 ]; then
    echo 'Run this installer with sudo.' >&2
    exit 1
fi
install -d -o root -g root -m 0755 /usr/bin /usr/share/bongocat /usr/share/applications
install -o root -g root -m 0755 usr/bin/bongocat-app /usr/bin/bongocat-app
install -o root -g root -m 0644 usr/share/applications/com.ayangweb.bongo-cat.desktop /usr/share/applications/
cp -R usr/share/bongocat/. /usr/share/bongocat/
chown -R root:root /usr/share/bongocat
chmod -R u=rwX,go=rX /usr/share/bongocat
