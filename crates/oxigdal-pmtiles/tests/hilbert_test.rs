//! Integration tests for Hilbert curve encoding in oxigdal-pmtiles.

#![allow(
    clippy::expect_used,
    clippy::useless_vec,
    clippy::manual_range_contains
)]

use oxigdal_pmtiles::{hilbert_to_xy, tile_id_to_zxy, xy_to_hilbert, zxy_to_tile_id};

// ── xy_to_hilbert / hilbert_to_xy ────────────────────────────────────────────

#[test]
fn test_hilbert_order0_identity() {
    assert_eq!(xy_to_hilbert(0, 0, 0), 0);
    assert_eq!(hilbert_to_xy(0, 0), (0, 0));
}

#[test]
fn test_hilbert_order1_forward() {
    // Order 1 (2x2): d=0->(0,0), d=1->(0,1), d=2->(1,1), d=3->(1,0)
    assert_eq!(xy_to_hilbert(0, 0, 1), 0);
    assert_eq!(xy_to_hilbert(0, 1, 1), 1);
    assert_eq!(xy_to_hilbert(1, 1, 1), 2);
    assert_eq!(xy_to_hilbert(1, 0, 1), 3);
}

#[test]
fn test_hilbert_order1_inverse() {
    assert_eq!(hilbert_to_xy(0, 1), (0, 0));
    assert_eq!(hilbert_to_xy(1, 1), (0, 1));
    assert_eq!(hilbert_to_xy(2, 1), (1, 1));
    assert_eq!(hilbert_to_xy(3, 1), (1, 0));
}

#[test]
fn test_hilbert_order2_round_trip_all() {
    for d in 0..16u64 {
        let (x, y) = hilbert_to_xy(d, 2);
        let d2 = xy_to_hilbert(x, y, 2);
        assert_eq!(d, d2, "round trip failed for d={d} => ({x},{y})");
    }
}

#[test]
fn test_hilbert_order3_round_trip_all() {
    for d in 0..64u64 {
        let (x, y) = hilbert_to_xy(d, 3);
        let d2 = xy_to_hilbert(x, y, 3);
        assert_eq!(d, d2, "round trip failed for d={d}");
    }
}

#[test]
fn test_hilbert_order4_round_trip_all() {
    for d in 0..256u64 {
        let (x, y) = hilbert_to_xy(d, 4);
        let d2 = xy_to_hilbert(x, y, 4);
        assert_eq!(d, d2);
    }
}

#[test]
fn test_hilbert_order5_round_trip_sample() {
    // Sample every 32nd point in a 32x32 grid
    for d in (0..1024u64).step_by(32) {
        let (x, y) = hilbert_to_xy(d, 5);
        assert!(x < 32 && y < 32, "out of range for order 5");
        let d2 = xy_to_hilbert(x, y, 5);
        assert_eq!(d, d2);
    }
}

#[test]
fn test_hilbert_order1_covers_all_cells() {
    let mut visited = [false; 4];
    for d in 0..4u64 {
        let (x, y) = hilbert_to_xy(d, 1);
        visited[(y * 2 + x) as usize] = true;
    }
    assert!(visited.iter().all(|&v| v), "Not all cells visited");
}

#[test]
fn test_hilbert_order2_covers_all_cells() {
    let mut visited = [false; 16];
    for d in 0..16u64 {
        let (x, y) = hilbert_to_xy(d, 2);
        visited[(y * 4 + x) as usize] = true;
    }
    assert!(visited.iter().all(|&v| v));
}

#[test]
fn test_hilbert_adjacency_order2() {
    // Consecutive Hilbert indices should differ by at most 1 step in x or y.
    for d in 0..15u64 {
        let (x1, y1) = hilbert_to_xy(d, 2);
        let (x2, y2) = hilbert_to_xy(d + 1, 2);
        let dist = (x1 as i32 - x2 as i32).unsigned_abs() + (y1 as i32 - y2 as i32).unsigned_abs();
        assert_eq!(
            dist, 1,
            "Non-adjacent at d={d}->d+1: ({x1},{y1})->({x2},{y2})"
        );
    }
}

// ── zxy_to_tile_id / tile_id_to_zxy ─────────────────────────────────────────

#[test]
fn test_zxy_tile_id_z0() {
    let id = zxy_to_tile_id(0, 0, 0).expect("valid");
    assert_eq!(id, 0);
    let (z, x, y) = tile_id_to_zxy(id).expect("valid");
    assert_eq!((z, x, y), (0, 0, 0));
}

#[test]
fn test_zxy_tile_id_z1_all_tiles() {
    for x in 0..2u32 {
        for y in 0..2u32 {
            let id = zxy_to_tile_id(1, x, y).expect("valid");
            let (z2, x2, y2) = tile_id_to_zxy(id).expect("valid");
            assert_eq!((1u8, x, y), (z2, x2, y2), "failed for (1,{x},{y})");
        }
    }
}

#[test]
fn test_zxy_tile_id_z2_all_tiles() {
    for x in 0..4u32 {
        for y in 0..4u32 {
            let id = zxy_to_tile_id(2, x, y).expect("valid");
            let (z2, x2, y2) = tile_id_to_zxy(id).expect("valid");
            assert_eq!((2u8, x, y), (z2, x2, y2));
        }
    }
}

#[test]
fn test_zxy_tile_id_z3_all_tiles() {
    for x in 0..8u32 {
        for y in 0..8u32 {
            let id = zxy_to_tile_id(3, x, y).expect("valid");
            let (z2, x2, y2) = tile_id_to_zxy(id).expect("valid");
            assert_eq!((3u8, x, y), (z2, x2, y2));
        }
    }
}

#[test]
fn test_tile_id_z1_range() {
    // z=1 tiles should have IDs 1..4 (offset for z=1 is 1, 4 tiles)
    let id_min = zxy_to_tile_id(1, 0, 0).expect("valid");
    assert!(id_min >= 1);
    for x in 0..2u32 {
        for y in 0..2u32 {
            let id = zxy_to_tile_id(1, x, y).expect("valid");
            assert!(
                (1..5).contains(&id),
                "z=1 tile id {id} out of expected range [1,5)"
            );
        }
    }
}

#[test]
fn test_tile_id_uniqueness_z3() {
    let mut ids = std::collections::HashSet::new();
    for x in 0..8u32 {
        for y in 0..8u32 {
            let id = zxy_to_tile_id(3, x, y).expect("valid");
            assert!(ids.insert(id), "duplicate id {id} for (3,{x},{y})");
        }
    }
    assert_eq!(ids.len(), 64);
}

#[test]
fn test_zxy_out_of_range_x() {
    assert!(zxy_to_tile_id(1, 2, 0).is_err());
}

#[test]
fn test_zxy_out_of_range_y() {
    assert!(zxy_to_tile_id(1, 0, 2).is_err());
}

#[test]
fn test_zxy_z0_nonzero_coords() {
    assert!(zxy_to_tile_id(0, 1, 0).is_err());
    assert!(zxy_to_tile_id(0, 0, 1).is_err());
}

#[test]
fn test_zxy_max_zoom_error() {
    assert!(zxy_to_tile_id(27, 0, 0).is_err());
    assert!(zxy_to_tile_id(28, 0, 0).is_err());
}

#[test]
fn test_zxy_zoom26_origin() {
    // Zoom 26 should work at (0,0)
    let id = zxy_to_tile_id(26, 0, 0).expect("valid");
    let (z, x, y) = tile_id_to_zxy(id).expect("valid");
    assert_eq!((z, x, y), (26, 0, 0));
}

#[test]
fn test_tile_id_monotonic_within_zoom() {
    // Tile IDs within a zoom level should all be in the same range
    let base = zxy_to_tile_id(2, 0, 0).expect("valid");
    for x in 0..4u32 {
        for y in 0..4u32 {
            let id = zxy_to_tile_id(2, x, y).expect("valid");
            assert!(id >= base);
            assert!(id < base + 16);
        }
    }
}

#[test]
fn test_zoom_levels_non_overlapping() {
    // IDs from different zoom levels should not overlap
    let z0_id = zxy_to_tile_id(0, 0, 0).expect("valid");
    let z1_ids: Vec<u64> = (0..2u32)
        .flat_map(|x| (0..2u32).map(move |y| zxy_to_tile_id(1, x, y).expect("valid")))
        .collect();
    let z2_ids: Vec<u64> = (0..4u32)
        .flat_map(|x| (0..4u32).map(move |y| zxy_to_tile_id(2, x, y).expect("valid")))
        .collect();

    assert!(!z1_ids.contains(&z0_id));
    for id in &z2_ids {
        assert!(!z1_ids.contains(id));
        assert_ne!(*id, z0_id);
    }
}

#[test]
fn test_tile_id_to_zxy_large_id() {
    // The tile ID just beyond max z=26 should fail
    // But a tile ID from z=10 should work fine
    let id = zxy_to_tile_id(10, 500, 300).expect("valid");
    let (z, x, y) = tile_id_to_zxy(id).expect("valid");
    assert_eq!((z, x, y), (10, 500, 300));
}
