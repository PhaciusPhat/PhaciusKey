PhaciusKey — Vietnamese Keyboard

Homepage source for [phacius_vnkey](https://github.com/PhaciusPhat/phacius_vnkey).
Published with GitHub Pages from the `main` branch, `/docs` folder:
https://phaciusphat.github.io/phacius_vnkey/

## Local preview

```bash
cd docs
bundle install       # needs Ruby + Bundler
bundle exec jekyll serve
```

## Releasing

Bump `version:` in `_config.yml` as part of every app release. The download
button and install steps build version-exact URLs from it
(`releases/download/v<version>/PhaciusKey-<version>.dmg`) rather than using
`releases/latest`, because the release workflow publishes with `--prerelease`
and GitHub's "latest" only ever points at a non-prerelease.

## Editing

- Page copy lives in `_includes/content.html` and `_includes/header.html`.
- Styles: edit `assets/css/main.css` directly. The `src/styles/*.scss` sources
  and `gulpfile.js` are kept for reference but the gulp 3 pipeline no longer
  runs on modern Node. (Reviving it also needs `src/fonts/` — copy it back from
  `assets/fonts/`; the duplicate wasn't worth committing.)
- `specs/` holds design documents and is excluded from the published site.

## Credits

Built on the *particle* Jekyll theme — MIT © 2017 Mauricio Urraco, see
`LICENSE`. Page structure adapted from
the EVKey homepage; all product copy is specific to PhaciusKey.
