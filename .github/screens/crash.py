# A crash report the simulator's app left (an .ips: a line of JSON, then the
# report's JSON), said in a few lines: the exception, the termination, and
# the crashed thread's top frames with their images (apps-ios.sh, when an
# app ends with its walk unsaid).
import json
import sys

text = open(sys.argv[1], errors="replace").read()
head, _, body = text.partition("\n")
try:
    r = json.loads(body)
except ValueError:
    print(text[:3000])
    sys.exit(0)
print("crash report:", sys.argv[1].rsplit("/", 1)[-1])
for k in ("exception", "termination", "asi", "lastExceptionBacktrace"):
    if k in r and k != "lastExceptionBacktrace":
        print(f"{k}: {json.dumps(r[k])[:600]}")
images = r.get("usedImages", [])
threads = r.get("threads", [])
t = r.get("faultingThread", 0)
frames = (r.get("lastExceptionBacktrace") or (threads[t]["frames"] if t < len(threads) else []))[:25]
for f in frames:
    img = images[f["imageIndex"]].get("name", "?") if f.get("imageIndex", -1) < len(images) else "?"
    print(f"  {img}: {f.get('symbol', hex(f.get('imageOffset', 0)))}" + (f" ({f['sourceFile']}:{f.get('sourceLine')})" if "sourceFile" in f else ""))
