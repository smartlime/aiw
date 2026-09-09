#!/bin/sh

set -eu

binary=$1
shift

case "$binary" in
    target/debug/aiw|target/release/aiw|*/target/debug/aiw|*/target/release/aiw)
        identity=${AIW_CODESIGN_IDENTITY:-aiw Local Code Signing}
        if ! signing_error=$(codesign --force --sign "$identity" --identifier com.smartlime.aiw "$binary" 2>&1); then
            printf '%s\n' "$signing_error" >&2
            exit 1
        fi
        ;;
esac

exec "$binary" "$@"
