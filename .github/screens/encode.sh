#!/usr/bin/env bash
# A recording made a page's MP4: sped back up by as many times as the
# device's animations were slowed while it was taken, so the motion is
# life's speed with that many times the frames the device could draw, then
# $fps frames a second, H.264 High, 540 wide, ready to play as it loads.
#
#   encode.sh <recording> <out.mp4> <slow> <fps>
#
# `slow` is how many times slower the whole recording was taken, or the
# file timewarp.c wrote of when the shot was told to go and when it changed
# (`<seconds> <factor>` by the host's clock), with the recording's first
# frame at the time record.swift wrote beside it (<recording>.start): each
# stretch sped up by its own, from the go, which ios.sh had the recorder
# take a frame at, to when the shot said it was done (`<seconds> end`).
#
# Says how many of its frames are distinct: a frame the device did not
# draw in time is the last one held, and this is how to see it.
set -euo pipefail
in=$1
out=$2
slow=$3
fps=$4
if [ -f "$slow" ]; then
  # Life's time at the recording's time T, in seconds from its first frame:
  # a piece for each stretch, from the change that began it, chosen by
  # if(lt(T,change),before,after) from the last change back.
  retime=$(awk -v start="$(cat "$in.start")" '
    $2 == "end" { done = $1 - start; next }
    { real[n + 0] = $1 - start; factor[n + 0] = $2; n++ }
    END {
      # Life at each change, from the go.
      for (i = 0; i < n; i++) at[i] = i ? at[i - 1] + (real[i] - real[i - 1]) / factor[i - 1] : 0
      expr = ""
      for (i = n - 1; i >= 0; i--) {
        piece = sprintf("(%.9f)+(T-(%.9f))/%g", at[i], real[i], factor[i])
        expr = expr == "" ? piece : sprintf("if(lt(T,%.9f),%s,%s)", real[i + 1], piece, expr)
      }
      # Before the go, a time before 0, which the output does not start from.
      print sprintf("if(lt(T,%.9f),T-(%.9f),%s)", real[0], real[0], expr)
      # Then how long it is: to the done, in life.
      if (done) {
        for (i = 0; i < n && real[i] <= done; i++) last = i
        printf "%.9f\n", at[last] + (done - real[last]) / factor[last]
      }
    }' "$slow")
  length=$(sed -n 2p <<< "$retime")
  retime=$(head -1 <<< "$retime")
  start=":start_time=0"
  slowest=$(awk 'BEGIN { m = 1 } $2 != "end" && $2 + 0 > m { m = $2 + 0 } END { print m }' "$slow")
else
  retime="T/$slow"
  slowest=$slow
  start=
  length=
fi
ffmpeg -nostdin -loglevel error -y -i "$in" \
  -vf "setpts='($retime)/TB',fps=$fps$start,scale=540:-2:flags=lanczos,format=yuv420p" \
  ${length:+-t "$length"} -an -c:v libx264 -preset slow -crf 22 -profile:v high -movflags +faststart "$out"
frames=$(ffprobe -v error -select_streams v:0 -show_entries stream=nb_frames -of csv=p=0 "$out")
seconds=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out")
distinct=$(ffmpeg -nostdin -i "$out" -vf mpdecimate -f null - 2>&1 | grep -o 'frame= *[0-9]*' | tail -1 | tr -dc 0-9)
echo "  $frames frames at $fps a second over ${seconds}s, $distinct distinct (moves taken $slowest times slower than life)"
