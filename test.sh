#!/usr/bin/env bash

set -e
DIR=$(realpath $0) && DIR=${DIR%/*}
cd $DIR
if [ -f "$DIR/sh/env.sh" ]; then
  . "$DIR/sh/env.sh"
fi
set -x
exec cargo nextest run --all-features "$@"
