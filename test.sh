#!/bin/zsh

./target/release/av1an -y -i /home/janek/Documents/GitHub/Av1an/testing/input.mkv \
    -e svt-av1 -v "--crf 23 --preset 3 --lp 2 --keyint -1 --enable-qm 1 --qm-min 0 --scm 0 --sharpness 2 --tune 3" \
    --verbose -x 240 --set-thread-affinity 2 --sc-downscale-height 480 \
    --chunk-method lsmash --concat mkvmerge --photon-noise 6  \
    --temp "/home/janek/Documents/GitHub/Av1an/testing/temp" \
    -w 16 -r --opus-mode -o /home/janek/Documents/GitHub/Av1an/testing/output.mkv