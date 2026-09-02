# Shipping the bridge with a release

The Homebrew formula installs `Cider Bridge.app` (WeatherKit, no HomeKit —
Apple does not grant that entitlement to Developer ID builds) and the
`cider-bridge` CLI from one notarized tarball. The release workflow attaches
it only if it is already on the release, so build and upload it by hand,
between steps 3 and 4 of the Release Process in `AGENTS.md`:

```sh
# 1. On the Mac with the Developer ID certificate and the notarytool profile
#    (`xcrun notarytool store-credentials alchemy-notary`, or set
#    CIDER_NOTARY_PROFILE). Version comes from Cargo.toml.
bridge/scripts/build.sh --distribution
#    -> bridge/dist/cider-bridge-X.Y.Z-macos-universal.tar.gz (+ sha256)

# 2. Upload onto the DRAFT release, before the tag exists (the tag triggers
#    the workflow; a published release is immutable and takes no more assets).
gh release upload vX.Y.Z bridge/dist/cider-bridge-X.Y.Z-macos-universal.tar.gz

# 3. Create the tag (AGENTS.md step 4). The workflow finds the asset, hashes
#    it, and passes bridge-url/bridge-sha256 to the tap; without it the
#    formula simply omits the bridge.
```

Users then get `<prefix>/opt/cider/libexec/Cider Bridge.app` and
`<prefix>/opt/cider/bin/cider-bridge`, which is where cider looks. Live
HomeKit still needs a personal build: `cider bridge build --install`.
`bridge/scripts/formula-example.rb` is what the generated formula looks like.
