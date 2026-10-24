import io

p = "crates/ops/src/params.rs"
s = io.open(p, encoding="utf-8").read()
n0 = len(s)

# vibrance/saturation arm -> Encoded (3D LUT on encoded domain)
old = """            Self::Vibrance { amount } | Self::Saturation { amount } => {
                // OKLab chroma ops are expensive per pixel; precompute a
                // 33³ display-domain LUT and sample trilinearly.
                if *amount == 0.0 {
                    Box::new(|px| px)
                } else {
                    let n = 33u32;
                    let op = self.clone();
                    let mut table = Vec::with_capacity((n * n * n) as usize);
                    for b in 0..n {
                        for g in 0..n {
                            for r in 0..n {
                                let e = [
                                    r as f32 / (n - 1) as f32,
                                    g as f32 / (n - 1) as f32,
                                    b as f32 / (n - 1) as f32,
                                ];
                                let lin = [
                                    srgb::decode(e[0]),
                                    srgb::decode(e[1]),
                                    srgb::decode(e[2]),
                                ];
                                let out = match &op {
                                    Self::Vibrance { amount } => {
                                        color::vibrance_pixel([lin[0], lin[1], lin[2], 1.0], *amount)
                                    }
                                    Self::Saturation { amount } => {
                                        color::saturation_pixel([lin[0], lin[1], lin[2], 1.0], *amount)
                                    }
                                    _ => unreachable!(),
                                };
                                table.push([
                                    srgb::encode_continuous(out[0]),
                                    srgb::encode_continuous(out[1]),
                                    srgb::encode_continuous(out[2]),
                                ]);
                            }
                        }
                    }
                    Box::new(move |px: Rgba32F| {
                        let e = [
                            srgb::encode_continuous(px[0]),
                            srgb::encode_continuous(px[1]),
                            srgb::encode_continuous(px[2]),
                        ];
                        let out = lut::sample_table(&table, n, e);
                        [
                            srgb::decode(out[0]),
                            srgb::decode(out[1]),
                            srgb::decode(out[2]),
                            px[3],
                        ]
                    })
                }
            }"""
new = """            Self::Vibrance { amount } | Self::Saturation { amount } => {
                // OKLab chroma ops are expensive per pixel; precompute a
                // 33³ encoded-domain LUT and sample trilinearly.
                if *amount == 0.0 {
                    Prepared::Encoded(Box::new(|e| e))
                } else {
                    let n = 33u32;
                    let op = self.clone();
                    let mut table = Vec::with_capacity((n * n * n) as usize);
                    for b in 0..n {
                        for g in 0..n {
                            for r in 0..n {
                                let e = [
                                    r as f32 / (n - 1) as f32,
                                    g as f32 / (n - 1) as f32,
                                    b as f32 / (n - 1) as f32,
                                ];
                                let lin = [
                                    srgb::decode(e[0]),
                                    srgb::decode(e[1]),
                                    srgb::decode(e[2]),
                                ];
                                let out = match &op {
                                    Self::Vibrance { amount } => {
                                        color::vibrance_pixel([lin[0], lin[1], lin[2], 1.0], *amount)
                                    }
                                    Self::Saturation { amount } => {
                                        color::saturation_pixel([lin[0], lin[1], lin[2], 1.0], *amount)
                                    }
                                    _ => unreachable!(),
                                };
                                table.push([
                                    srgb::encode_continuous(out[0]),
                                    srgb::encode_continuous(out[1]),
                                    srgb::encode_continuous(out[2]),
                                ]);
                            }
                        }
                    }
                    Prepared::Encoded(Box::new(move |e: [f32; 3]| lut::sample_table(&table, n, e)))
                }
            }"""
assert old in s, "vib/sat arm"
s = s.replace(old, new, 1)

# levels arm -> Encoded
old = """            Self::Levels { black, gamma, white } => {
                if self.is_identity() {
                    return Box::new(|px| px);
                }
                let (black, gamma, white) = (*black, *gamma, *white);
                let mut t = [0.0f32; PREPARED_LUT_N];
                for (j, slot) in t.iter_mut().enumerate() {
                    *slot = levels::levels_encoded(j as f32 / (PREPARED_LUT_N - 1) as f32, black, gamma, white);
                }
                Box::new(move |px: Rgba32F| {
                    let t = Some(t);
                    let e = [
                        apply_table(srgb::encode_continuous(px[0]), &t),
                        apply_table(srgb::encode_continuous(px[1]), &t),
                        apply_table(srgb::encode_continuous(px[2]), &t),
                    ];
                    [
                        srgb::decode(e[0]),
                        srgb::decode(e[1]),
                        srgb::decode(e[2]),
                        px[3],
                    ]
                })
            }"""
new = """            Self::Levels { black, gamma, white } => {
                if self.is_identity() {
                    return Prepared::Encoded(Box::new(|e| e));
                }
                let (black, gamma, white) = (*black, *gamma, *white);
                let mut t = [0.0f32; PREPARED_LUT_N];
                for (j, slot) in t.iter_mut().enumerate() {
                    *slot = levels::levels_encoded(
                        j as f32 / (PREPARED_LUT_N - 1) as f32,
                        black,
                        gamma,
                        white,
                    );
                }
                Prepared::Encoded(Box::new(move |e: [f32; 3]| {
                    [
                        apply_table(e[0], &Some(t)),
                        apply_table(e[1], &Some(t)),
                        apply_table(e[2], &Some(t)),
                    ]
                }))
            }"""
assert old in s, "levels arm"
s = s.replace(old, new, 1)

# ToneCurve arm -> Encoded
old = """            Self::ToneCurve { r, g, b, l } => {
                let mut tables: [Option<[f32; PREPARED_LUT_N]>; 4] = [None, None, None, None];
                for (i, pts) in [r, g, b, l].iter().enumerate() {
                    if pts.is_empty()
                        || curves::MonotoneSpline::is_identity_points(pts)
                    {
                        continue;
                    }
                    if let Some(spline) = curves::MonotoneSpline::new(pts.to_vec()) {
                        let mut t = [0.0f32; PREPARED_LUT_N];
                        for (j, slot) in t.iter_mut().enumerate() {
                            *slot = spline.eval(j as f32 / (PREPARED_LUT_N - 1) as f32);
                        }
                        tables[i] = Some(t);
                    }
                }
                if tables.iter().all(|t| t.is_none()) {
                    return Box::new(|px| px);
                }
                Box::new(move |px: Rgba32F| {
                    // Encode to the (continuous) sRGB domain, apply per-channel
                    // curves and the luma curve, decode back.
                    let e = [
                        srgb::encode_continuous(px[0]),
                        srgb::encode_continuous(px[1]),
                        srgb::encode_continuous(px[2]),
                    ];
                    let mut e = [
                        apply_table(e[0], &tables[0]),
                        apply_table(e[1], &tables[1]),
                        apply_table(e[2], &tables[2]),
                    ];
                    if let Some(lt) = tables[3].as_ref() {
                        let y = 0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2];
                        let y2 = apply_table_opt(y, Some(lt));
                        let factor = y2 / y.max(1e-3);
                        e = [
                            (e[0] * factor).clamp(0.0, 1.0),
                            (e[1] * factor).clamp(0.0, 1.0),
                            (e[2] * factor).clamp(0.0, 1.0),
                        ];
                    }
                    [
                        srgb::decode(e[0]),
                        srgb::decode(e[1]),
                        srgb::decode(e[2]),
                        px[3],
                    ]
                })
            }"""
new = """            Self::ToneCurve { r, g, b, l } => {
                let mut tables: [Option<[f32; PREPARED_LUT_N]>; 4] = [None, None, None, None];
                for (i, pts) in [r, g, b, l].iter().enumerate() {
                    if pts.is_empty() || curves::MonotoneSpline::is_identity_points(pts) {
                        continue;
                    }
                    if let Some(spline) = curves::MonotoneSpline::new(pts.to_vec()) {
                        let mut t = [0.0f32; PREPARED_LUT_N];
                        for (j, slot) in t.iter_mut().enumerate() {
                            *slot = spline.eval(j as f32 / (PREPARED_LUT_N - 1) as f32);
                        }
                        tables[i] = Some(t);
                    }
                }
                if tables.iter().all(|t| t.is_none()) {
                    return Prepared::Encoded(Box::new(|e| e));
                }
                Prepared::Encoded(Box::new(move |e: [f32; 3]| {
                    let mut e = [
                        apply_table(e[0], &tables[0]),
                        apply_table(e[1], &tables[1]),
                        apply_table(e[2], &tables[2]),
                    ];
                    if let Some(lt) = tables[3].as_ref() {
                        let y = 0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2];
                        let y2 = apply_table_opt(y, Some(lt));
                        let factor = y2 / y.max(1e-3);
                        e = [
                            (e[0] * factor).clamp(0.0, 1.0),
                            (e[1] * factor).clamp(0.0, 1.0),
                            (e[2] * factor).clamp(0.0, 1.0),
                        ];
                    }
                    e
                }))
            }"""
assert old in s, "curve arm"
s = s.replace(old, new, 1)

# fallback arm -> Linear
old = """            other => {
                let op = other.clone();
                Box::new(move |px: Rgba32F| op.apply_pixel(px))
            }
        }
    }
}"""
new = """            other => {
                let op = other.clone();
                Prepared::Linear(Box::new(move |px: Rgba32F| op.apply_pixel(px)))
            }
        }
    }
}"""
assert old in s, "fallback arm"
s = s.replace(old, new, 1)

io.open(p, "w", encoding="utf-8", newline="").write(s)
print("ok", n0, "->", len(s))
