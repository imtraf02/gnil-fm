# gnil-fm project rules

## Visual assets

- Reserve AI image generation for the application logo only. The production logo must contain no
  letters, words, monograms or wordmark.
- Author every functional UI icon as native SVG. Do not generate folder, file-kind, toolbar, status or
  permission icons as raster images.
- Use a `24 24` viewBox for UI icons and verify that the silhouette remains legible at 16 px.
- Keep icons flat and two-tone: sage `#8ca894` and graphite `#172019`, plus transparency. Do not use
  gradients, drop shadows, glow, textures, blur, lighting or 3D shading.
- Prefer simple filled geometry and hard edges. Keep semantic badges sparse and do not encode an app or
  vendor logo into a file icon.
- File icons represent kinds only: generic, code, text, image, document, archive and media.
- Folder icons represent states only: closed, open, favorite, symlink and readonly.
- Build empty-state artwork as SVG. Export PNG/WebP only when a distribution or documentation target
  requires raster output; keep the SVG as the source of truth.
- Keep AI source provenance and export metadata in `assets/brand/manifest.json`.
