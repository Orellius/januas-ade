# UPSTREAM_NOTES.md

Vendored fork of [glyphon 0.11.0](https://github.com/grovesNL/glyphon) at the
crates.io-published source. Tri-licensed `MIT OR Apache-2.0 OR Zlib` upstream;
inherited verbatim.

## Why vendored

Glyphon's fragment shader does a plain `color × mask.x` for masked glyphs.
That mode is gamma-correct in linear sRGB, but it produces visibly thinner
light-on-dark text on retina than Warp / Windows Terminal / iTerm2, which all
apply a DirectWrite-style contrast curve before the alpha multiply.

Glyphon has no public hook to substitute the shader (it's `include_str!`'d at
crate-compile time). Vendoring is the only way to land the patch without
upstreaming first.

## Diff from upstream (2026-05-16, slice S5.5d.5)

One file changed: `src/shader.wgsl`. Two new helper functions and three
fragment-stage lines.

```diff
+// Brightness-scaled contrast enhancement for glyph alpha masks.
+//
+// enhance_contrast() adapted from DWrite_EnhanceContrast in Windows
+// Terminal's DirectWrite shader (MIT-licensed):
+//   https://github.com/microsoft/terminal/blob/.../src/renderer/atlas/dwrite.hlsl
+// Lifted verbatim from Warp's MIT-licensed glyph_shader.wgsl:
+//   crates/warpui/src/rendering/wgpu/shaders/glyph_shader.wgsl
+fn glyph_color_brightness(color: vec3<f32>) -> f32 {
+    return dot(color, vec3<f32>(0.30, 0.59, 0.11));
+}
+
+fn enhance_contrast(alpha: f32, k: f32) -> f32 {
+    return alpha * (k + 1.0) / (alpha * k + 1.0);
+}
```

And the fragment's masked-glyph case (content_type == 1u):

```diff
 case 1u: {
-    return vec4<f32>(in_frag.color.rgb, in_frag.color.a * textureSampleLevel(mask_atlas_texture, atlas_sampler, in_frag.uv, 0.0).x);
+    let mask = textureSampleLevel(mask_atlas_texture, atlas_sampler, in_frag.uv, 0.0).x;
+    let k = glyph_color_brightness(in_frag.color.rgb);
+    let contrasted = enhance_contrast(mask, k);
+    return vec4<f32>(in_frag.color.rgb, in_frag.color.a * contrasted);
 }
```

Color glyphs (content_type == 0u, emoji) are untouched — they already carry
their own RGB; contrast enhancement would shift their hue.

## Re-syncing with upstream

When glyphon publishes 0.12+:

1. `cp -R ~/.cargo/registry/src/.../glyphon-0.12.0/src/* crates/januas-glyphon/src/`
2. Re-apply the two diffs above to `src/shader.wgsl`.
3. Bump `wgpu` / `cosmic-text` / `lru` / `rustc-hash` in this `Cargo.toml` to
   match upstream's new constraints.
4. Smoke `cargo run --release --example home_compose -p januas-ui`.

If the upstream shader has been restructured (uniforms, content-type encoding,
mask vs color path), re-apply the patch against the new structure rather than
copy-pasting blindly. The two helper functions stay verbatim; the fragment
edit is structural.

## Rename: `glyphon` → `januas_glyphon`

Package name + lib name renamed so the workspace path dep doesn't shadow the
registry's `glyphon` (we still pull the registry version through other
transitive paths — `cosmic-text` is shared). Public API is otherwise identical;
the only call-site change is `use glyphon::...` → `use januas_glyphon::...`.

## Read-aloud pass

Every public function in this crate is glyphon's, not ours, and is documented
upstream. The patch surface is two small functions; both summarize in one
sentence ("REC.601 brightness of a linear RGB triple" and "DirectWrite
contrast-curve evaluation"). No AI slop merged.
