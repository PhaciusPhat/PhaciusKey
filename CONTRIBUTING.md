# Contributing to PhaciusKey

## Building from source

```sh
cargo build            # debug build of everything
cargo run -p vnkey     # run the app (menu-bar accessory; needs Accessibility)
cargo test             # engine + app tests
```

To build a distributable DMG for yourself (no release certificate):

```sh
PHACIUSKEY_ALLOW_ADHOC=1 bash scripts/package-app.sh
```

An ad-hoc build works fine, but its code-signing identity changes every build,
so *its own* Accessibility grant resets after each auto-update. Published
releases don't have that problem — see below.

## Releasing

CI does the work: push a version tag and a signed `PhaciusKey-<version>.dmg` is
published to a GitHub Release via `.github/workflows/release.yml`.

Every release is signed with **one shared identity, `phaciuskey-release`**, so
users keep their Accessibility grant across auto-updates — macOS keys the grant
to the signing certificate (its SHA-1), not to the app's name or path. The
canonical certificate is pinned by SHA-1 in `scripts/package-app.sh`
(`RELEASE_CERT_SHA`); a DMG signed by anything else refuses to build. Two certs
that share the name are still *different* identities, so never recreate the
cert casually — losing it means every user re-grants Accessibility once on the
next update.

### One-time setup (repo admin)

1. Create the self-signed code-signing certificate (or reuse the existing
   `phaciuskey-release.p12`):

   ```sh
   openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 3650 -nodes \
     -subj "/CN=phaciuskey-release" \
     -addext "keyUsage=critical,digitalSignature" \
     -addext "extendedKeyUsage=critical,codeSigning" \
     -addext "basicConstraints=critical,CA:FALSE"
   openssl pkcs12 -export -out phaciuskey-release.p12 -inkey key.pem -in cert.pem \
     -name "phaciuskey-release"        # choose an export password
   ```

2. Add two GitHub repository secrets (Settings → Secrets and variables →
   Actions):
   - `RELEASE_CERT_P12_BASE64` — `base64 -i phaciuskey-release.p12 | pbcopy`
   - `RELEASE_CERT_PASSWORD` — the export password from step 1

3. If the certificate ever changes, update `RELEASE_CERT_SHA` in
   `scripts/package-app.sh` to the new SHA-1
   (`security find-identity -p codesigning`).

4. Keep the `.p12` and its password somewhere safe (password manager). Anyone
   holding it can sign builds that inherit users' Accessibility grants — treat
   it like a credential, and never commit it (`.signing/` is gitignored for
   local material).

### Cutting a release

1. Bump the version in **both** `apps/vnkey/Cargo.toml` and
   `apps/vnkey/Info.plist` (`CFBundleShortVersionString`), run `cargo build` so
   `Cargo.lock` follows, and commit.
2. Tag and push:

   ```sh
   git tag v0.0.11 && git push origin v0.0.11
   ```

CI verifies the tag matches both version fields, signs the DMG with
`phaciuskey-release`, and publishes the GitHub Release. The workflow refuses to
ship if the signing secret is missing — an ad-hoc "release" would silently
reset every user's Accessibility grant.

Running apps notice the new release within a day (checks run at launch and
daily thereafter), download it, swap it in place, and relaunch.

### Releasing from a local machine (fallback)

With the `phaciuskey-release` identity imported in a keychain that is in your
user search list and unlocked:

```sh
SIGN_KEYCHAIN=/path/to/release.keychain-db bash scripts/package-app.sh
gh release create v<version> dist/PhaciusKey-<version>.dmg \
  --title "PhaciusKey <version>" --generate-notes --prerelease
```

The repository must stay public (or org-accessible) — the in-app updater reads
the releases feed anonymously.
