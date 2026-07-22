# Asset direction

gnil-fm uses a restrained vector visual system. AI image generation is reserved for the application
logo; functional icons and empty-state artwork are authored as native SVG:

- graphite `#171b18` for structure;
- sage `#8ca894` for folders and focused state;
- off-white `#ecf0ed` for legibility;
- flat geometry, strong silhouettes and generous negative space;
- no gradients, glass, neon, mascots, decorative gloss or dense detail.

Generated source images live in `assets/brand/generated/` and `assets/icons/generated/`. Production
brand derivatives include a transparent master plus 512, 256, 128, 64 and 32 px PNGs. Production UI
icons are hand-simplified SVGs with exactly two paint colors, hard geometry, no effects and a 24 × 24
viewBox designed to survive at 16 px. The generated PNG icon sizes are retained as source-history raster
exports. The empty state ships as SVG, PNG and lossless WebP. Small sizes must be inspected rather than
blindly downscaled: use one dark outline, at most two interior planes, and remove details that collapse
below 32 px.

The generation prompt and provenance are recorded in `assets/brand/manifest.json`. Folder icons encode
state (closed, open, favorite, symlink and readonly); file icons encode kind rather than a specific app.
The logo contains no visible text or letterform. The SVG mark is a deterministic fallback for packaging
and environments where the raster set is unavailable.
