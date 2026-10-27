#!/usr/bin/env bash
# A recording made a page's MP4: sped back up by as many times as the
# device's animations were slowed while it was taken, so the motion is
# life's speed with that many times the frames the device could draw, then
# $fps frames a second, H.264 High, 540 wide, ready to play as it loads.
#
#   encode.sh <recording> <out.mp4> <slow> <fps>
#
# Says how many of its frames are distinct: a frame the device did not
# draw in time is the last one held, and this is how to see it.
set -euo pipefail
in=$1
out=$2
slow=$3
fps=$4
ffmpeg -nostdin -loglevel error -y -i "$in" \
  -vf "setpts=PTS/$slow,fps=$fps,scale=540:-2:flags=lanczos,format=yuv420p" \
  -an -c:v libx264 -preset slow -crf 22 -profile:v high -movflags +faststart "$out"
frames=$(ffprobe -v error -select_streams v:0 -show_entries stream=nb_frames -of csv=p=0 "$out")
seconds=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out")
distinct=$(ffmpeg -nostdin -i "$out" -vf mpdecimate -f null - 2>&1 | grep -o 'frame= *[0-9]*' | tail -1 | tr -dc 0-9)
echo "  $frames frames at $fps a second over ${seconds}s, $distinct distinct (taken $slow times slower than life)"
