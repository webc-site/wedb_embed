#!/usr/bin/env bash

set -e
DIR=$(realpath $0) && DIR=${DIR%/*}
cd $DIR
# set -a
# . ../../conf/env/xxx.env
# set +a
set -x

cargo nextest run -p fastalp --all-features --no-capture
