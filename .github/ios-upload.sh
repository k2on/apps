#!/usr/bin/env bash
# The archive `$RUNNER_TEMP/<App>.xcarchive` signed for the App Store and
# sent to App Store Connect (workflows/ios.yaml says how): the API key
# written from ASC_KEY_P8, the export's options, and xcodebuild's export with
# `destination: upload`. With DESTINATION=export it is signed the same way
# and written out as an .ipa, sent nowhere: what a branch's run checks.
set -euo pipefail
app=$1

mkdir -p "$RUNNER_TEMP/asc"
key="$RUNNER_TEMP/asc/AuthKey_$ASC_KEY_ID.p8"
printf '%s' "$ASC_KEY_P8" > "$key"
chmod 600 "$key"

cat > "$RUNNER_TEMP/ExportOptions.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>app-store-connect</string>
  <key>destination</key><string>${DESTINATION:-upload}</string>
  <key>teamID</key><string>$APPLE_TEAM_ID</string>
  <key>signingStyle</key><string>automatic</string>
  <key>uploadSymbols</key><true/>
  <key>manageAppVersionAndBuildNumber</key><false/>
</dict>
</plist>
PLIST

xcodebuild -exportArchive \
  -archivePath "$RUNNER_TEMP/$app.xcarchive" \
  -exportOptionsPlist "$RUNNER_TEMP/ExportOptions.plist" \
  -exportPath "$RUNNER_TEMP/export" \
  -allowProvisioningUpdates \
  -authenticationKeyPath "$key" \
  -authenticationKeyID "$ASC_KEY_ID" \
  -authenticationKeyIssuerID "$ASC_ISSUER_ID"
