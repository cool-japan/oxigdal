//! Integration tests for the dynamic rendering pipeline.
//!
//! Covers shader hot-reload, tile compositing, blend modes, colour matrices,
//! and the high-level `TileRenderPipeline`.

use oxigdal_gpu::compositing::{
    BlendMode, ColorMatrix, Layer, Rgba, TileCompositor, TileRenderPipeline,
};
use oxigdal_gpu::shader_reload::{HotReloadRegistry, ShaderStage, ShaderWatcher};

const EPS: f32 = 1e-4;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < EPS
}

fn approx_rgba(a: Rgba, b: Rgba) -> bool {
    approx(a.r, b.r) && approx(a.g, b.g) && approx(a.b, b.b) && approx(a.a, b.a)
}

// ── Rgba round-trips ──────────────────────────────────────────────────────────

#[test]
fn test_from_u8_to_u8_round_trip() {
    for val in [0_u8, 1, 127, 128, 200, 255] {
        let px = Rgba::from_u8(val, val, val, val);
        let (r, g, b, a) = px.to_u8();
        assert_eq!(r, val, "r channel failed for {val}");
        assert_eq!(g, val, "g channel failed for {val}");
        assert_eq!(b, val, "b channel failed for {val}");
        assert_eq!(a, val, "a channel failed for {val}");
    }
}

#[test]
fn test_from_u8_normalises_channels() {
    let px = Rgba::from_u8(255, 0, 128, 255);
    assert!(approx(px.r, 1.0));
    assert!(approx(px.g, 0.0));
    assert!((px.b - 128.0 / 255.0).abs() < 1e-3);
    assert!(approx(px.a, 1.0));
}

#[test]
fn test_to_u8_clamps_over() {
    let px = Rgba::new(2.0, -1.0, 0.5, 1.0);
    let (r, g, b, _) = px.to_u8();
    assert_eq!(r, 255);
    assert_eq!(g, 0);
    assert_eq!(b, 128);
}

#[test]
fn test_clamp_all_channels() {
    let px = Rgba::new(1.5, -0.5, 0.5, 2.0).clamp();
    assert!(approx(px.r, 1.0));
    assert!(approx(px.g, 0.0));
    assert!(approx(px.b, 0.5));
    assert!(approx(px.a, 1.0));
}

// ── premultiply / unpremultiply ───────────────────────────────────────────────

#[test]
fn test_premultiply_scales_rgb() {
    let px = Rgba::new(1.0, 0.5, 0.25, 0.5);
    let pre = px.premultiply();
    assert!(approx(pre.r, 0.5));
    assert!(approx(pre.g, 0.25));
    assert!(approx(pre.b, 0.125));
    assert!(approx(pre.a, 0.5));
}

#[test]
fn test_premultiply_full_alpha_noop() {
    let px = Rgba::new(0.3, 0.6, 0.9, 1.0);
    let pre = px.premultiply();
    assert!(approx(pre.r, px.r));
    assert!(approx(pre.g, px.g));
    assert!(approx(pre.b, px.b));
}

#[test]
fn test_unpremultiply_recovers_straight() {
    let px = Rgba::new(0.5, 0.25, 0.125, 0.5);
    let straight = px.unpremultiply();
    assert!(approx(straight.r, 1.0));
    assert!(approx(straight.g, 0.5));
    assert!(approx(straight.b, 0.25));
}

#[test]
fn test_unpremultiply_zero_alpha_returns_transparent() {
    let px = Rgba::new(0.5, 0.5, 0.5, 0.0);
    let straight = px.unpremultiply();
    assert!(approx(straight.a, 0.0));
    assert!(approx(straight.r, 0.0));
}

#[test]
fn test_premultiply_unpremultiply_round_trip() {
    let px = Rgba::new(0.8, 0.4, 0.2, 0.6);
    let recovered = px.premultiply().unpremultiply();
    assert!(approx_rgba(px, recovered));
}

// ── BlendMode::Normal / SrcOver ───────────────────────────────────────────────

#[test]
fn test_blend_normal_opaque_src_replaces_dst() {
    let src = Rgba::new(1.0, 0.0, 0.0, 1.0);
    let dst = Rgba::new(0.0, 1.0, 0.0, 1.0);
    let result = BlendMode::Normal.blend(src, dst);
    assert!(approx_rgba(result, src));
}

#[test]
fn test_blend_normal_transparent_src_passes_through() {
    let src = Rgba::new(1.0, 0.0, 0.0, 0.0);
    let dst = Rgba::new(0.0, 1.0, 0.0, 1.0);
    let result = BlendMode::Normal.blend(src, dst);
    assert!(approx_rgba(result, dst));
}

#[test]
fn test_blend_src_over_half_alpha_formula() {
    let src = Rgba::new(1.0, 0.0, 0.0, 0.5);
    let dst = Rgba::new(0.0, 0.0, 1.0, 1.0);
    let result = BlendMode::SrcOver.blend(src, dst);
    assert!(approx(result.a, 1.0));
    assert!(approx(result.r, 0.5));
    assert!(approx(result.b, 0.5));
}

#[test]
fn test_blend_normal_equals_src_over() {
    let src = Rgba::new(0.3, 0.5, 0.7, 0.6);
    let dst = Rgba::new(0.9, 0.1, 0.4, 0.8);
    let a = BlendMode::Normal.blend(src, dst);
    let b = BlendMode::SrcOver.blend(src, dst);
    assert!(approx_rgba(a, b));
}

// ── BlendMode::Multiply ───────────────────────────────────────────────────────

#[test]
fn test_blend_multiply_black_src_gives_black() {
    let src = Rgba::new(0.0, 0.0, 0.0, 1.0);
    let dst = Rgba::new(0.8, 0.5, 0.3, 1.0);
    let result = BlendMode::Multiply.blend(src, dst);
    assert!(approx(result.r, 0.0));
    assert!(approx(result.g, 0.0));
    assert!(approx(result.b, 0.0));
}

#[test]
fn test_blend_multiply_white_src_identity() {
    let src = Rgba::new(1.0, 1.0, 1.0, 1.0);
    let dst = Rgba::new(0.5, 0.5, 0.5, 1.0);
    let result = BlendMode::Multiply.blend(src, dst);
    // 1.0 * 0.5 = 0.5.
    assert!(approx(result.r, 0.5));
}

// ── BlendMode::Screen ─────────────────────────────────────────────────────────

#[test]
fn test_blend_screen_white_src_gives_white() {
    let src = Rgba::new(1.0, 1.0, 1.0, 1.0);
    let dst = Rgba::new(0.5, 0.5, 0.5, 1.0);
    let result = BlendMode::Screen.blend(src, dst);
    assert!(approx(result.r, 1.0));
    assert!(approx(result.g, 1.0));
    assert!(approx(result.b, 1.0));
}

#[test]
fn test_blend_screen_black_src_noop() {
    let src = Rgba::new(0.0, 0.0, 0.0, 1.0);
    let dst = Rgba::new(0.6, 0.3, 0.9, 1.0);
    let result = BlendMode::Screen.blend(src, dst);
    assert!(approx(result.r, 0.6));
    assert!(approx(result.g, 0.3));
    assert!(approx(result.b, 0.9));
}

// ── BlendMode::Darken / Lighten ───────────────────────────────────────────────

#[test]
fn test_blend_darken_picks_minimum_channel() {
    let src = Rgba::new(0.3, 0.9, 0.5, 1.0);
    let dst = Rgba::new(0.7, 0.2, 0.8, 1.0);
    let result = BlendMode::Darken.blend(src, dst);
    // Each channel ≤ both inputs.
    assert!(result.r <= src.r + EPS);
    assert!(result.r <= dst.r + EPS);
    assert!(result.g <= src.g + EPS);
    assert!(result.g <= dst.g + EPS);
}

#[test]
fn test_blend_lighten_picks_maximum_channel() {
    let src = Rgba::new(0.3, 0.9, 0.5, 1.0);
    let dst = Rgba::new(0.7, 0.2, 0.8, 1.0);
    let result = BlendMode::Lighten.blend(src, dst);
    assert!(result.r >= src.r - EPS || result.r >= dst.r - EPS);
}

// ── BlendMode::Difference ─────────────────────────────────────────────────────

#[test]
fn test_blend_difference_same_color_gives_black() {
    let color = Rgba::new(0.6, 0.3, 0.9, 1.0);
    let result = BlendMode::Difference.blend(color, color);
    assert!(approx(result.r, 0.0));
    assert!(approx(result.g, 0.0));
    assert!(approx(result.b, 0.0));
}

#[test]
fn test_blend_difference_opposite_channels() {
    let src = Rgba::new(1.0, 0.0, 0.5, 1.0);
    let dst = Rgba::new(0.0, 1.0, 0.5, 1.0);
    let result = BlendMode::Difference.blend(src, dst);
    assert!(approx(result.r, 1.0));
    assert!(approx(result.g, 1.0));
    assert!(approx(result.b, 0.0));
}

// ── Porter-Duff operators ─────────────────────────────────────────────────────

#[test]
fn test_porter_duff_src_over_opaque_dst() {
    let src = Rgba::new(1.0, 0.0, 0.0, 0.5);
    let dst = Rgba::new(0.0, 1.0, 0.0, 1.0);
    let result = BlendMode::SrcOver.blend(src, dst);
    assert!(approx(result.a, 1.0));
}

#[test]
fn test_porter_duff_src_in_alpha_product() {
    let src = Rgba::new(1.0, 0.0, 0.0, 0.5);
    let dst = Rgba::new(0.0, 1.0, 0.0, 0.8);
    let result = BlendMode::SrcIn.blend(src, dst);
    // a_out = 0.5 * 0.8 = 0.4
    assert!(approx(result.a, 0.4));
}

#[test]
fn test_porter_duff_src_out_opaque_dst_zero() {
    let src = Rgba::new(1.0, 0.0, 0.0, 1.0);
    let dst = Rgba::new(0.0, 1.0, 0.0, 1.0);
    let result = BlendMode::SrcOut.blend(src, dst);
    assert!(approx(result.a, 0.0));
}

#[test]
fn test_porter_duff_clear_always_transparent() {
    let src = Rgba::new(0.9, 0.8, 0.7, 1.0);
    let dst = Rgba::new(0.1, 0.2, 0.3, 0.9);
    let result = BlendMode::Clear.blend(src, dst);
    assert!(approx(result.a, 0.0));
    assert!(approx(result.r, 0.0));
}

#[test]
fn test_porter_duff_xor_both_opaque_gives_transparent() {
    let src = Rgba::new(1.0, 0.0, 0.0, 1.0);
    let dst = Rgba::new(0.0, 1.0, 0.0, 1.0);
    let result = BlendMode::Xor.blend(src, dst);
    assert!(approx(result.a, 0.0));
}

// ── Layer ─────────────────────────────────────────────────────────────────────

#[test]
fn test_layer_fill_sets_all_pixels() {
    let color = Rgba::new(0.1, 0.2, 0.3, 0.4);
    let layer = Layer::new("base", 8, 8).fill(color);
    assert_eq!(layer.pixels.len(), 64);
    for px in &layer.pixels {
        assert!(approx_rgba(*px, color));
    }
}

#[test]
fn test_layer_pixel_at_returns_correct_value() {
    let layer = Layer::new("l", 3, 3).fill(Rgba::white());
    let px = layer.pixel_at(1, 1);
    assert!(px.is_some());
    assert!(approx_rgba(px.expect("should exist"), Rgba::white()));
}

#[test]
fn test_layer_pixel_at_out_of_bounds_returns_none() {
    let layer = Layer::new("l", 4, 4);
    assert!(layer.pixel_at(4, 0).is_none());
    assert!(layer.pixel_at(0, 4).is_none());
    assert!(layer.pixel_at(100, 100).is_none());
}

#[test]
fn test_layer_set_pixel_in_bounds() {
    let mut layer = Layer::new("l", 4, 4);
    let ok = layer.set_pixel(2, 3, Rgba::white());
    assert!(ok);
    assert!(approx_rgba(
        layer.pixel_at(2, 3).expect("should be Some"),
        Rgba::white()
    ));
}

#[test]
fn test_layer_set_pixel_out_of_bounds_returns_false() {
    let mut layer = Layer::new("l", 4, 4);
    assert!(!layer.set_pixel(4, 0, Rgba::white()));
    assert!(!layer.set_pixel(0, 4, Rgba::white()));
}

// ── TileCompositor ────────────────────────────────────────────────────────────

#[test]
fn test_compositor_z_order_lower_is_bottom() {
    let comp = TileCompositor::new(1, 1, Rgba::transparent());
    let mut bottom = Layer::new("b", 1, 1).fill(Rgba::new(0.0, 0.0, 1.0, 1.0));
    bottom.z_order = 0;
    let mut top = Layer::new("t", 1, 1).fill(Rgba::new(1.0, 0.0, 0.0, 1.0));
    top.z_order = 1;
    let mut layers = vec![top, bottom]; // reverse insertion order
    let out = comp.composite(&mut layers);
    assert!(approx(out[0].r, 1.0));
    assert!(approx(out[0].b, 0.0));
}

#[test]
fn test_compositor_invisible_layer_ignored() {
    let comp = TileCompositor::new(1, 1, Rgba::black());
    let mut inv = Layer::new("inv", 1, 1).fill(Rgba::white());
    inv.visible = false;
    let mut layers = vec![inv];
    let out = comp.composite(&mut layers);
    assert!(approx_rgba(out[0], Rgba::black()));
}

#[test]
fn test_compositor_opacity_halves_alpha() {
    let comp = TileCompositor::new(1, 1, Rgba::transparent());
    let mut layer = Layer::new("l", 1, 1).fill(Rgba::new(1.0, 0.0, 0.0, 1.0));
    layer.opacity = 0.5;
    let mut layers = vec![layer];
    let out = comp.composite(&mut layers);
    assert!(approx(out[0].a, 0.5));
}

#[test]
fn test_compositor_to_rgba_bytes_correct_length() {
    let comp = TileCompositor::new(8, 8, Rgba::transparent());
    let mut layers: Vec<Layer> = vec![];
    let pixels = comp.composite(&mut layers);
    let bytes = TileCompositor::to_rgba_bytes(&pixels);
    assert_eq!(bytes.len(), 8 * 8 * 4);
}

#[test]
fn test_compositor_to_rgb_bytes_correct_length() {
    let pixels = vec![Rgba::white(); 16];
    let bytes = TileCompositor::to_rgb_bytes(&pixels);
    assert_eq!(bytes.len(), 48);
}

// ── ColorMatrix ───────────────────────────────────────────────────────────────

#[test]
fn test_color_matrix_identity_is_noop() {
    let m = ColorMatrix::identity();
    let px = Rgba::new(0.4, 0.6, 0.2, 0.8);
    assert!(approx_rgba(m.apply(px), px));
}

#[test]
fn test_color_matrix_brightness_2x_doubles_rgb() {
    let m = ColorMatrix::brightness(2.0);
    let px = Rgba::new(0.2, 0.3, 0.4, 1.0);
    let out = m.apply(px);
    assert!(approx(out.r, 0.4));
    assert!(approx(out.g, 0.6));
    assert!(approx(out.b, 0.8));
}

#[test]
fn test_color_matrix_invert_flips_channels() {
    let m = ColorMatrix::invert();
    let px = Rgba::new(0.2, 0.5, 0.8, 1.0);
    let out = m.apply(px);
    assert!(approx(out.r, 0.8));
    assert!(approx(out.g, 0.5));
    assert!(approx(out.b, 0.2));
    assert!(approx(out.a, 1.0));
}

#[test]
fn test_color_matrix_grayscale_equal_rgb_channels() {
    let m = ColorMatrix::grayscale();
    let px = Rgba::new(0.6, 0.4, 0.2, 1.0);
    let out = m.apply(px);
    assert!(approx(out.r, out.g));
    assert!(approx(out.g, out.b));
}

#[test]
fn test_color_matrix_compose_identity_commutative() {
    let any = ColorMatrix::brightness(1.5);
    let left = ColorMatrix::identity().compose(&any);
    let right = any.compose(&ColorMatrix::identity());
    let px = Rgba::new(0.3, 0.5, 0.7, 1.0);
    assert!(approx_rgba(left.apply(px), any.apply(px)));
    assert!(approx_rgba(right.apply(px), any.apply(px)));
}

#[test]
fn test_color_matrix_invert_composed_is_identity() {
    let composed = ColorMatrix::invert().compose(&ColorMatrix::invert());
    let id = ColorMatrix::identity();
    let px = Rgba::new(0.3, 0.6, 0.9, 0.7);
    assert!(approx_rgba(composed.apply(px), id.apply(px)));
}

// ── ShaderWatcher ─────────────────────────────────────────────────────────────

#[test]
fn test_shader_watcher_add_inline_get_source() {
    let mut w = ShaderWatcher::new(100);
    w.add_inline("my_shader", "@compute fn main() {}");
    let src = w.get_source("my_shader");
    assert!(src.is_some());
    assert_eq!(src.expect("should exist").label, "my_shader");
}

#[test]
fn test_shader_watcher_initial_version_is_one() {
    let mut w = ShaderWatcher::new(100);
    w.add_inline("s", "@compute fn main() {}");
    assert_eq!(w.source_version("s"), Some(1));
}

#[test]
fn test_shader_watcher_update_source_increments_version() {
    let mut w = ShaderWatcher::new(100);
    w.add_inline("s", "@compute fn main() {}");
    let ok = w.update_source("s", "@compute fn main_v2() {}");
    assert!(ok);
    assert_eq!(w.source_version("s"), Some(2));
}

#[test]
fn test_shader_watcher_update_unknown_returns_false() {
    let mut w = ShaderWatcher::new(100);
    assert!(!w.update_source("ghost", "fn x() {}"));
}

#[test]
fn test_shader_watcher_poll_changes_after_update() {
    let mut w = ShaderWatcher::new(100);
    w.add_inline("s", "@compute fn main() {}");
    let _ = w.poll_changes(); // sync snapshot
    w.update_source("s", "@compute fn main_v2() {}");
    let events = w.poll_changes();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].label, "s");
    assert_eq!(events[0].old_version, 1);
    assert_eq!(events[0].new_version, 2);
}

#[test]
fn test_shader_watcher_poll_second_time_empty() {
    let mut w = ShaderWatcher::new(100);
    w.add_inline("s", "fn main() {}");
    w.update_source("s", "fn main_v2() {}");
    let _ = w.poll_changes();
    assert!(w.poll_changes().is_empty());
}

#[test]
fn test_shader_entry_point_stage() {
    use oxigdal_gpu::shader_reload::EntryPoint;
    let ep = EntryPoint::new("vs_main", ShaderStage::Vertex);
    assert_eq!(ep.stage, ShaderStage::Vertex);
    assert_eq!(ep.name, "vs_main");
}

// ── HotReloadRegistry ─────────────────────────────────────────────────────────

#[test]
fn test_registry_process_changes_invalidates_pipeline() {
    let mut reg = HotReloadRegistry::new();
    reg.watcher.add_inline("shader_a", "@compute fn main() {}");
    reg.register_pipeline("pipeline_1", "shader_a");
    reg.watcher.poll_changes();

    reg.watcher
        .update_source("shader_a", "@compute fn main_v2() {}");
    let invalidated = reg.process_changes();
    assert!(invalidated.contains(&"pipeline_1".to_owned()));
    assert!(reg.is_invalidated("pipeline_1"));
}

#[test]
fn test_registry_unrelated_shader_no_invalidation() {
    let mut reg = HotReloadRegistry::new();
    reg.watcher.add_inline("shader_a", "fn a() {}");
    reg.watcher.add_inline("shader_b", "fn b() {}");
    reg.register_pipeline("pipeline_a", "shader_a");
    reg.watcher.poll_changes();

    reg.watcher.update_source("shader_b", "fn b_v2() {}");
    let invalidated = reg.process_changes();
    assert!(!invalidated.contains(&"pipeline_a".to_owned()));
}

#[test]
fn test_registry_clear_invalidated() {
    let mut reg = HotReloadRegistry::new();
    reg.watcher.add_inline("s", "fn main() {}");
    reg.register_pipeline("p", "s");
    reg.watcher.poll_changes();
    reg.watcher.update_source("s", "fn main_v2() {}");
    reg.process_changes();
    assert!(reg.is_invalidated("p"));
    reg.clear_invalidated("p");
    assert!(!reg.is_invalidated("p"));
}

// ── TileRenderPipeline ─────────────────────────────────────────────────────────

#[test]
fn test_pipeline_render_byte_count() {
    let pipeline = TileRenderPipeline::new(4, 4);
    let mut layers = vec![Layer::new("l", 4, 4).fill(Rgba::white())];
    let bytes = pipeline.render(&mut layers);
    assert_eq!(bytes.len(), 4 * 4 * 4);
}

#[test]
fn test_pipeline_render_with_brightness_matrix() {
    let pipeline = TileRenderPipeline::new(2, 2);
    let mut layers = vec![Layer::new("l", 2, 2).fill(Rgba::new(0.3, 0.3, 0.3, 1.0))];
    let matrix = ColorMatrix::brightness(2.0);
    let bytes = pipeline.render_with_matrix(&mut layers, &matrix);
    // 0.3 * 2 = 0.6 → 153 in u8.
    assert_eq!(bytes.len(), 16);
    assert_eq!(bytes[0], 153);
}

// ── CompositeStats ────────────────────────────────────────────────────────────

#[test]
fn test_composite_stats_mean_r_manual() {
    let pixels = vec![Rgba::new(0.2, 0.0, 0.0, 1.0), Rgba::new(0.6, 0.0, 0.0, 1.0)];
    let stats = TileCompositor::stats(&pixels);
    // mean_r = (0.2 + 0.6) / 2 = 0.4
    assert!(approx(stats.mean_r, 0.4));
}

#[test]
fn test_composite_stats_transparent_count() {
    let pixels = vec![
        Rgba::new(0.0, 0.0, 0.0, 0.0),
        Rgba::new(0.0, 0.0, 0.0, 0.0),
        Rgba::new(1.0, 1.0, 1.0, 1.0),
    ];
    let stats = TileCompositor::stats(&pixels);
    assert_eq!(stats.transparent_pixel_count, 2);
}
