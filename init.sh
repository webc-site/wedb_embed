#!/usr/bin/env bash

DIR=$(realpath $0) && DIR=${DIR%/*}
cd $DIR
set -ex

if [ -d "sh" ]; then
  cd sh
  git pull
  cd ..
else
  local_share="$HOME/.local/share"
  sh="$local_share/cargo_sh"
  if [ ! -d "$sh" ]; then
    mkdir -p $local_share
    cd $local_share
    git clone --depth=1 ssh://git@ssh.github.com:443/i18n-site/cargo_sh.git
  else
    cd $sh
    git pull
  fi
  cd $DIR
  ln -s $sh sh
fi

./sh/init.sh
