# Archify 2D Source

`a3s-cloud.architecture.json` is generated from the same typed A3S graph used
by the Three.js scene. The checked-in HTML artifact is rendered with
[Archify](https://github.com/tt-a1i/archify) 2.12.0 under its MIT license.

Regenerate the typed source after changing architecture nodes, domains, or
relationships:

```bash
bun run archify:source
```

Then render and validate it with an Archify 2.12.0 checkout:

```bash
node "$ARCHIFY_HOME/bin/archify.mjs" deliver architecture \
  archify/a3s-cloud.architecture.json \
  public/archify/a3s-cloud.architecture.html \
  --quality standard \
  --json
```

The application embeds the self-contained artifact from `public/archify/` and
bridges Archify node and authored relationship deep links back to the shared
A3S HUD.
