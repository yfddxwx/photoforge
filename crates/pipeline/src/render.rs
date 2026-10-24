//! The non-destructive renderer.
//!
//! Two execution paths (ADR-0006):
//! - **Preview** (`render_level`): renders one pyramid level, caching the
//!   result of every stage (each enabled adjustment) as 16-bit linear
//!   images, keyed by the FNV hash of the adjustment prefix that produced
//!   them — editing adjustment *k* only recomputes stages > *k*.
//! - **Export** (`render_full`): replays the whole stack at full resolution
//!   in one fused pass per band (no intermediate quantization), then applies
//!   geometry.
//!
//! Masked adjustments blend per pixel *inside* the chain:
//! `out = lerp(in, op(in), mask)` — no fusion break required.

use std::collections::HashMap;
use std::sync::Arc;

use photoforge_core::math::lerp;
use photoforge_core::tile::par_bands;
use photoforge_core::{LinearImage, LinearImageQ16, PixelBuffer};
use photoforge_io::ImagePyramid;

use crate::geometry::Quality;
use crate::mask::Mask;
use crate::state::{Adjustment, EditState};

/// Default stage-cache memory budget.
const DEFAULT_BUDGET_BYTES: usize = 512 * 1024 * 1024;

/// Non-destructive renderer owning the stage/mask/final caches.
pub struct Renderer {
    state: EditState,
    /// Per-adjustment revision (bumped whenever that adjustment changes).
    revs: Vec<u64>,
    /// Geometry revision.
    geom_rev: u64,
    cache: StageCache,
    pub stats: CacheStats,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub final_hits: u64,
    pub final_misses: u64,
}

impl Renderer {
    pub fn new(state: EditState) -> Self {
        let n = state.adjustments.len();
        Self {
            state,
            revs: vec![1; n],
            geom_rev: 1,
            cache: StageCache::new(DEFAULT_BUDGET_BYTES),
            stats: CacheStats::default(),
        }
    }

    pub fn state(&self) -> &EditState {
        &self.state
    }

    /// Adopts a (possibly edited) state, bumping revisions only where the
    /// content actually changed so unaffected caches stay valid.
    pub fn sync_state(&mut self, new: &EditState) {
        if self.state.adjustments.len() != new.adjustments.len() {
            // Structural change: bump everything (fresh revision space).
            let base = self.geom_rev.wrapping_mul(1_000_003);
            self.revs = (0..new.adjustments.len())
                .map(|i| base.wrapping_add(i as u64))
                .collect();
        } else {
            for (i, (old, new_adj)) in self
                .state
                .adjustments
                .iter()
                .zip(new.adjustments.iter())
                .enumerate()
            {
                if old != new_adj {
                    self.revs[i] = self.revs[i].wrapping_add(1);
                }
            }
        }
        if self.state.geometry != new.geometry {
            self.geom_rev = self.geom_rev.wrapping_add(1);
        }
        self.state = new.clone();
    }

    /// Enabled adjustments in pipeline order, paired with their original
    /// indices (for revision lookups). Owned clones so cache mutations can
    /// borrow `self` freely (OpParams/Mask clones are cheap).
    pub fn active_adjustments(&self) -> Vec<(usize, Adjustment)> {
        self.state
            .adjustments
            .iter()
            .enumerate()
            .filter(|(_, a)| a.enabled)
            .map(|(i, a)| (i, a.clone()))
            .collect()
    }

    /// Renders a pyramid level through the full adjustment stack + geometry.
    pub fn render_level(
        &mut self,
        pyramid: &ImagePyramid,
        level: usize,
        quality: Quality,
    ) -> Arc<PixelBuffer> {
        let active = self.active_adjustments();
        let prefix = PrefixHashes::build(&active, &self.revs);
        let final_hash = prefix
            .full
            .combine_u64(self.geom_rev)
            .combine_u64(quality as u64)
            .combine_u64(pyramid.level(level).width() as u64)
            .0;

        let final_key = CacheKey::Final {
            level: level as u32,
            quality,
        };
        if let Some(entry) = self.cache.get(&final_key) {
            if entry.hash == final_hash {
                self.stats.final_hits += 1;
                if let Payload::Final(buf) = &entry.payload {
                    return buf.clone();
                }
            }
        }
        self.stats.final_misses += 1;

        let w = pyramid.level(level).width();
        let h = pyramid.level(level).height();
        let n = active.len();

        // Deepest cached stage with a matching prefix hash.
        // Stage i holds the image after applying `i` active ops, so after
        // loading stage i the next op to run is index i (not i+1).
        let mut img: Option<Arc<LinearImageQ16>> = None;
        let mut start = 0usize;
        for i in 0..=n {
            let key = CacheKey::Stage {
                level: level as u32,
                stage: i,
            };
            let want = prefix.at(i);
            match self.cache.get(&key) {
                Some(entry) if entry.hash == want => {
                    if let Payload::Stage(q) = &entry.payload {
                        img = Some(q.clone());
                        start = i;
                    }
                }
                _ => break,
            }
        }
        let mut img = img.unwrap_or_else(|| {
            let base = LinearImageQ16::from_pixel_buffer(&pyramid.level(level));
            let base = Arc::new(base);
            self.cache_put(
                CacheKey::Stage {
                    level: level as u32,
                    stage: 0,
                },
                prefix.at(0),
                Payload::Stage(base.clone()),
            );
            base
        });

        // Execute in SEGMENTS: runs of pointwise ops fuse into one pass
        // (with one Q16 materialization at the segment end); spatial ops are
        // their own segment. Caching happens at segment ends — an edit to op
        // k invalidates its segment and everything after (prefix hashes).
        let mut idx = start;
        while idx < n {
            let (_, adj) = &active[idx];
            if adj.op.is_pointwise() {
                // Find the end of this pointwise run.
                let mut end = idx + 1;
                while end < n && active[end].1.op.is_pointwise() {
                    end += 1;
                }
                // Build fused units (masked ops stay individual inside the run).
                let ops_with_masks: Vec<(photoforge_ops::OpParams, Option<Arc<Vec<f32>>>)> = active
                    [idx..end]
                    .iter()
                    .map(|(_, adj)| {
                        let mask = adj
                            .mask
                            .as_ref()
                            .map(|m| self.mask_raster(m, &adj.id, w, h));
                        (adj.op.clone(), mask)
                    })
                    .collect();
                let lin = img.to_linear();
                let out = apply_pointwise_units(&lin, &ops_with_masks);
                img = Arc::new(LinearImageQ16::from_linear(&out));
                idx = end; // the whole run applied at once
            } else {
                let mask = adj
                    .mask
                    .as_ref()
                    .map(|m| self.mask_raster(m, &adj.id, w, h));
                let next = apply_adjustment(&img, adj.op.clone(), mask);
                img = Arc::new(next);
                idx += 1;
            }
            self.cache_put(
                CacheKey::Stage {
                    level: level as u32,
                    stage: idx,
                },
                prefix.at(idx),
                Payload::Stage(img.clone()),
            );
        }
        let mut result = img.to_pixel_buffer();
        if !self.state.geometry.is_identity() {
            result = self.state.geometry.apply(&result, quality);
        }
        let result = Arc::new(result);
        self.cache_put(final_key, final_hash, Payload::Final(result.clone()));
        result
    }

    /// Full-resolution export: pointwise runs fuse into single passes;
    /// spatial ops split the chain and run whole-image (ADR-0003/0006), then
    /// geometry.
    pub fn render_full(&mut self, pyramid: &ImagePyramid, quality: Quality) -> PixelBuffer {
        let base = pyramid.level(0);
        let active = self.active_adjustments();
        let (w, h) = (base.width(), base.height());
        // Owned op+mask list (masks rastereized at full res, cached).
        let mut ops: Vec<(photoforge_ops::OpParams, Option<Arc<Vec<f32>>>)> = active
            .iter()
            .map(|(_, adj)| {
                let mask = adj
                    .mask
                    .as_ref()
                    .map(|m| self.mask_raster(m, &adj.id, w, h));
                (adj.op.clone(), mask)
            })
            .collect();

        let mut cur = LinearImage::from_pixel_buffer(&base);
        let mut segment: Vec<(photoforge_ops::OpParams, Option<Arc<Vec<f32>>>)> = Vec::new();
        for (op, mask) in ops.drain(..) {
            if op.is_pointwise() {
                segment.push((op, mask));
            } else {
                if !segment.is_empty() {
                    cur = run_pointwise_segment(&cur, &segment);
                    segment.clear();
                }
                // Spatial op with optional mask blend.
                let mut next = if mask.is_some() {
                    cur.clone()
                } else {
                    std::mem::replace(&mut cur, LinearImage::new(0, 0))
                };
                op.apply_image(&mut next);
                if let Some(m) = &mask {
                    // Blend every pixel: mask 0 restores the pre-op value.
                    for (i, &mv) in m.iter().enumerate() {
                        let mv = mv.clamp(0.0, 1.0);
                        for c in 0..4 {
                            let j = i * 4 + c;
                            next.data[j] = lerp(cur.data[j], next.data[j], mv);
                        }
                    }
                }
                cur = next;
            }
        }
        if !segment.is_empty() {
            cur = run_pointwise_segment(&cur, &segment);
        }
        let u8img = cur.to_pixel_buffer();
        if self.state.geometry.is_identity() {
            u8img
        } else {
            self.state.geometry.apply(&u8img, quality)
        }
    }

    fn mask_raster(&mut self, mask: &Mask, adj_id: &str, w: u32, h: u32) -> Arc<Vec<f32>> {
        let key = CacheKey::Mask {
            adj: adj_id.to_string(),
            w,
            h,
        };
        let hash = hash_mask(mask);
        if let Some(entry) = self.cache.get(&key) {
            if entry.hash == hash {
                if let Payload::Mask(m) = &entry.payload {
                    return m.clone();
                }
            }
        }
        let raster = Arc::new(mask.rasterize(w, h));
        let bytes = raster.len() * 4;
        self.cache
            .insert(key, hash, Payload::Mask(raster.clone()), bytes);
        raster
    }

    fn cache_put(&mut self, key: CacheKey, hash: u64, payload: Payload) {
        let bytes = match &payload {
            Payload::Stage(q) => q.size_bytes(),
            Payload::Final(p) => p.len_bytes(),
            Payload::Mask(m) => m.len() * 4,
        };
        self.cache.insert(key, hash, payload, bytes);
    }
}

/// Applies one adjustment (pointwise op + optional mask blend) to a Q16
/// stage buffer, row-parallel.
/// Runs a fused segment of consecutive pointwise ops (with optional per-op
/// mask blends) over the whole image, out-of-place.
fn run_pointwise_segment(
    src: &LinearImage,
    segment: &[(photoforge_ops::OpParams, Option<Arc<Vec<f32>>>)],
) -> LinearImage {
    let (w, h) = (src.width, src.height);
    let mut out = LinearImage::new(w, h);
    let stride = w as usize * 4;
    let src_ref = src;
    // Build fused units: unmasked runs share one encode/decode round-trip
    // (ops::fuse_run); masked ops stay individual (their blend needs the
    // per-op input).
    let units = build_units(segment);
    par_bands(&mut out.data, stride, 32, |y0, y1, band| {
        for y in y0..y1 {
            let srow = &src_ref.data[y as usize * stride..(y as usize + 1) * stride];
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            for (x, o) in orow.chunks_exact_mut(4).enumerate() {
                let mut q = [
                    srow[x * 4],
                    srow[x * 4 + 1],
                    srow[x * 4 + 2],
                    srow[x * 4 + 3],
                ];
                for (fused, mask) in &units {
                    let p = q;
                    q = fused(p);
                    if let Some(m) = mask {
                        let mv = m[y as usize * w as usize + x];
                        if mv <= 0.0 {
                            q = p;
                        } else if mv < 1.0 {
                            for c in 0..4 {
                                q[c] = lerp(p[c], q[c], mv);
                            }
                        }
                    }
                }
                o.copy_from_slice(&q);
            }
        }
    });
    out
}

/// Applies fused pointwise units over a linear-light image, out-of-place.
fn apply_pointwise_units(
    src: &LinearImage,
    ops_with_masks: &[(photoforge_ops::OpParams, Option<Arc<Vec<f32>>>)],
) -> LinearImage {
    let units = build_units(ops_with_masks);
    let (w, h) = (src.width, src.height);
    let mut out = LinearImage::new(w, h);
    let stride = w as usize * 4;
    let src_ref = src;
    par_bands(&mut out.data, stride, 32, |y0, y1, band| {
        for y in y0..y1 {
            let srow = &src_ref.data[y as usize * stride..(y as usize + 1) * stride];
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            for (x, o) in orow.chunks_exact_mut(4).enumerate() {
                let mut q = [
                    srow[x * 4],
                    srow[x * 4 + 1],
                    srow[x * 4 + 2],
                    srow[x * 4 + 3],
                ];
                for (fused, mask) in &units {
                    let p = q;
                    q = fused(p);
                    if let Some(m) = mask {
                        let mv = m[y as usize * w as usize + x];
                        if mv <= 0.0 {
                            q = p;
                        } else if mv < 1.0 {
                            for c in 0..4 {
                                q[c] = lerp(p[c], q[c], mv);
                            }
                        }
                    }
                }
                o.copy_from_slice(&q);
            }
        }
    });
    out
}

/// One fused evaluation unit: a `fuse_run` over a run of unmasked pointwise
/// ops, or a single masked op, plus its optional mask raster.
type Unit = (photoforge_ops::params::PixelFn, Option<Arc<Vec<f32>>>);

fn build_units(segment: &[(photoforge_ops::OpParams, Option<Arc<Vec<f32>>>)]) -> Vec<Unit> {
    let mut units = Vec::new();
    let mut run: Vec<photoforge_ops::Prepared> = Vec::new();
    for (op, mask) in segment {
        if mask.is_none() {
            run.push(op.prepare());
        } else {
            if !run.is_empty() {
                units.push((photoforge_ops::fuse_run(std::mem::take(&mut run)), None));
            }
            units.push((op.prepare_pixel_fn(), mask.clone()));
        }
    }
    if !run.is_empty() {
        units.push((photoforge_ops::fuse_run(run), None));
    }
    units
}

fn apply_adjustment(
    src: &LinearImageQ16,
    op: photoforge_ops::OpParams,
    mask: Option<Arc<Vec<f32>>>,
) -> LinearImageQ16 {
    if op.is_identity() && mask.is_none() {
        return src.clone();
    }
    let (w, h) = (src.width, src.height);
    // Spatial ops run whole-image; masks blend against the pre-op pixels.
    if !op.is_pointwise() {
        let mut lin = src.to_linear();
        if mask.is_some() {
            let original = lin.clone();
            op.apply_image(&mut lin);
            if let Some(m) = &mask {
                // Blend EVERY pixel (mask 0 must restore the original —
                // the spatial op ran over the whole frame).
                for (i, &mv) in m.iter().enumerate() {
                    let mv = mv.clamp(0.0, 1.0);
                    for c in 0..4 {
                        let j = i * 4 + c;
                        lin.data[j] = lerp(original.data[j], lin.data[j], mv);
                    }
                }
            }
        } else {
            op.apply_image(&mut lin);
        }
        return LinearImageQ16::from_linear(&lin);
    }
    let mut dst = LinearImageQ16::new(w, h);
    let stride = w as usize * 4;
    let src_ref = src;
    par_bands(dst.data.as_mut_slice(), stride, 64, |y0, y1, band| {
        for y in y0..y1 {
            let srow = src_ref.row(y);
            let off = (y - y0) as usize * stride;
            let orow = &mut band[off..off + stride];
            let mrow = mask
                .as_deref()
                .map(|m| &m[y as usize * w as usize..(y as usize + 1) * w as usize]);
            for (x, o) in orow.chunks_exact_mut(4).enumerate() {
                let p = [
                    f32::from(srow[x * 4]) / 65535.0,
                    f32::from(srow[x * 4 + 1]) / 65535.0,
                    f32::from(srow[x * 4 + 2]) / 65535.0,
                    f32::from(srow[x * 4 + 3]) / 65535.0,
                ];
                let mut q = op.apply_pixel(p);
                if let Some(m) = mrow {
                    let mv = m[x];
                    if mv <= 0.0 {
                        q = p;
                    } else if mv < 1.0 {
                        for c in 0..4 {
                            q[c] = lerp(p[c], q[c], mv);
                        }
                    }
                }
                for c in 0..4 {
                    o[c] = (q[c].clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
                }
            }
        }
    });
    dst
}

// ---------------------------------------------------------------------------
// Prefix hashing
// ---------------------------------------------------------------------------

/// FNV-1a combining over the active adjustment sequence. `at(i)` is the hash
/// of the first `i` active adjustments — a cached stage is valid iff its
/// stored hash equals the current prefix hash (any edit/add/remove/enable
/// change upstream invalidates everything after it).
struct PrefixHashes {
    hashes: Vec<u64>,
    full: Hasher64,
}

impl PrefixHashes {
    /// `active` pairs each enabled adjustment with its index into
    /// `state.adjustments` (so `revs[orig_idx]` is its revision).
    fn build(active: &[(usize, Adjustment)], revs: &[u64]) -> Self {
        let mut hashes = Vec::with_capacity(active.len() + 1);
        let mut h = Hasher64::new();
        hashes.push(h.0);
        for &(idx, ref adj) in active {
            h = h
                .combine_str(&adj.id)
                .combine_u64(revs.get(idx).copied().unwrap_or(0));
            hashes.push(h.0);
        }
        Self { hashes, full: h }
    }

    fn at(&self, i: usize) -> u64 {
        self.hashes[i.min(self.hashes.len() - 1)]
    }
}

#[derive(Clone, Copy)]
struct Hasher64(u64);

impl Hasher64 {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn combine_u64(self, v: u64) -> Self {
        let mut h = self.0;
        for b in v.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self(h)
    }
    fn combine_str(self, s: &str) -> Self {
        let mut h = self;
        for b in s.as_bytes() {
            h.0 ^= u64::from(*b);
            h.0 = h.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

fn hash_mask(mask: &Mask) -> u64 {
    match mask {
        Mask::Brush { strokes } => {
            let mut h = Hasher64::new().combine_u64(1);
            for s in strokes {
                for p in &s.points {
                    h = h
                        .combine_u64(u64::from(p[0].to_bits()))
                        .combine_u64(u64::from(p[1].to_bits()));
                }
                h = h
                    .combine_u64(u64::from(s.radius.to_bits()))
                    .combine_u64(u64::from(s.hardness.to_bits()))
                    .combine_u64(u64::from(s.flow.to_bits()))
                    .combine_u64(u64::from(s.erase));
            }
            h.0
        }
        Mask::LinearGradient { start, end } => {
            Hasher64::new()
                .combine_u64(2)
                .combine_u64(u64::from(start[0].to_bits()))
                .combine_u64(u64::from(start[1].to_bits()))
                .combine_u64(u64::from(end[0].to_bits()))
                .combine_u64(u64::from(end[1].to_bits()))
                .0
        }
        Mask::RadialGradient {
            center,
            radius,
            feather,
        } => {
            Hasher64::new()
                .combine_u64(3)
                .combine_u64(u64::from(center[0].to_bits()))
                .combine_u64(u64::from(center[1].to_bits()))
                .combine_u64(u64::from(radius[0].to_bits()))
                .combine_u64(u64::from(radius[1].to_bits()))
                .combine_u64(u64::from(feather.to_bits()))
                .0
        }
    }
}

// ---------------------------------------------------------------------------
// Cache storage
// ---------------------------------------------------------------------------

#[derive(Clone, Hash, PartialEq, Eq)]
enum CacheKey {
    Stage { level: u32, stage: usize },
    Final { level: u32, quality: Quality },
    Mask { adj: String, w: u32, h: u32 },
}

enum Payload {
    Stage(Arc<LinearImageQ16>),
    Final(Arc<PixelBuffer>),
    Mask(Arc<Vec<f32>>),
}

struct Entry {
    hash: u64,
    payload: Payload,
    bytes: usize,
    last_used: u64,
}

struct StageCache {
    entries: HashMap<CacheKey, Entry>,
    budget_bytes: usize,
    used_bytes: usize,
    clock: u64,
}

impl StageCache {
    fn new(budget_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            budget_bytes,
            used_bytes: 0,
            clock: 0,
        }
    }

    fn get(&mut self, key: &CacheKey) -> Option<&Entry> {
        self.clock += 1;
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used = self.clock;
            Some(entry)
        } else {
            None
        }
    }

    fn insert(&mut self, key: CacheKey, hash: u64, payload: Payload, bytes: usize) {
        if bytes > self.budget_bytes {
            return; // single entry too big to cache
        }
        self.clock += 1;
        if let Some(old) = self.entries.insert(
            key,
            Entry {
                hash,
                payload,
                bytes,
                last_used: self.clock,
            },
        ) {
            self.used_bytes -= old.bytes;
        }
        self.used_bytes += bytes;
        self.evict();
    }

    fn evict(&mut self) {
        while self.used_bytes > self.budget_bytes {
            let victim = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone());
            match victim {
                Some(k) => {
                    if let Some(e) = self.entries.remove(&k) {
                        self.used_bytes -= e.bytes;
                    }
                }
                None => break,
            }
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Renderer {
    /// Number of cached entries (test/debug hook).
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
}
