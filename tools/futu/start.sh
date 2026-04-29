#!/bin/bash
set -e

OPEND_DIR="/Users/hayhay2323/Downloads/Futu_OpenD_10.2.6208_Mac/Futu_OpenD_10.2.6208_Mac"
OPEND_BIN="${OPEND_DIR}/FutuOpenD.app/Contents/MacOS/FutuOpenD"

if [ ! -x "${OPEND_BIN}" ]; then
  echo "FutuOpenD binary not found at ${OPEND_BIN}"
  exit 1
fi

cd "${OPEND_DIR}"
exec "${OPEND_BIN}"
