# 0002 — Color pipeline: linear working space and op domains

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Photo adjustments must behave like physical light operations (exposure is a
multiplier, white balance is a channel-gain adaptation, saturation lives in a
perceptual space). Meanwhile the canonical interchange format for display is
8-bit sRGB-encoded pixels, and tools like curve editors have a well-understood
meaning tied to the *encoded* value (the diagonal of a curve editor maps
encoded 0→0, encoded 1→1).

## Decision

1. **Storage:** `PixelBuffer` = 8-bit sRGB-encoded RGBA (straight alpha,
   treated as opaque by the pipeline).
2. **Working space:** `LinearImage` = f32 RGBA, linear light, nominal range
   [0, 1] (intermediates may exceed it; ops clamp on output). u8 → linear uses
   an exact 256-entry table; linear → u8 is clamp + encode + round. The u8
   round-trip must be bit-exact (tested for all 256 values).
3. **Op domains:**
   - Linear domain: exposure, contrast (pivot = 18% linear gray), highlights /
     shadows / whites / blacks, white balance (channel gains), saturation and
     vibrance (OKLab chroma), HSL luminance, blur/sharpen/clarity/denoise
     (linear before conversion to/from u8), grain, vignette, B&W channel
     mixing, duotone.
   - sRGB-encoded domain: tone curves, levels, `.cube` LUT application. The
     op converts linear→encoded→apply→linear at its boundary. Rationale: curve
     editors are authored and understood in the display-encoded domain
     (matching Lightroom/Capture One); applying them in linear would make a
     "filmic S-curve" look wrong and the LUT import semantically broken (cube
     LUTs are authored in the display domain).
4. **Luma:** Rec. 709 coefficients (0.2126, 0.7152, 0.0722) applied to
   *linear* RGB. Histograms expose per-channel bins of the encoded values plus
   an encoded-luma channel.
5. **OKLab** (Ottosson 2020, https://bottosson.github.io/posts/oklab/) is the
   perceptual space for chroma work; matrices are embedded with attribution.

## Consequences

- Gamma handling is concentrated in two conversion functions with
  boundary-condition tests (0, 1, clamp, round-trip exactness).
- Curve/levels ops carry a small encode/decode cost per pixel; acceptable and
  fusion-friendly.
- Anyone comparing raw sliders against other software should expect small
  differences by design; the invariant tests (monotone response, identity at
  defaults, region selectivity) define correctness instead.
