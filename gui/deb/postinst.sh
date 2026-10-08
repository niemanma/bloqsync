#!/bin/sh
set -e
udevadm control --reload-rules || true
udevadm trigger --subsystem-match=hidraw || true
udevadm trigger --subsystem-match=input || true
exit 0
