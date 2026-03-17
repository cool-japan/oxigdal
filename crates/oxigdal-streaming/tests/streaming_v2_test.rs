//! Integration tests for Streaming v2 features.
//!
//! Covers:
//!  - CreditPool / Backpressure  (15 tests)
//!  - SessionWindow              (15 tests)
//!  - StreamJoin                 (15 tests)
//!  - Checkpoint                 (15 tests)

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use oxigdal_streaming::v2::backpressure::{BackpressureConsumer, BackpressureProducer, CreditPool};
use oxigdal_streaming::v2::checkpoint::{
    CheckpointId, CheckpointManager, CheckpointState, InMemoryCheckpointStore,
};
use oxigdal_streaming::v2::session_window::{
    SessionWindowConfig, SessionWindowProcessor, StreamEvent,
};
use oxigdal_streaming::v2::stream_join::{JoinEvent, JoinMode, TemporalJoinConfig, TemporalJoiner};

// ─── helpers ─────────────────────────────────────────────────────────────────

fn ts(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn stream_event(secs: u64, seq: u64) -> StreamEvent<u64> {
    StreamEvent::new(ts(secs), seq, seq)
}

fn left(secs: u64, key: &str) -> JoinEvent<String> {
    JoinEvent::new(ts(secs), key, format!("L@{secs}"))
}

fn right(secs: u64, key: &str) -> JoinEvent<String> {
    JoinEvent::new(ts(secs), key, format!("R@{secs}"))
}

// ═══════════════════════════════════════════════════════════════════════════════
// §1  CreditPool / Backpressure
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn bp_pool_construction_initial_credits_correct() {
    let pool = CreditPool::new(200);
    assert_eq!(pool.available(), 200);
    assert_eq!(pool.capacity(), 200);
}

#[test]
fn bp_pool_try_acquire_succeeds_when_credits_available() {
    let pool = CreditPool::new(100);
    assert!(pool.try_acquire(50));
    assert_eq!(pool.available(), 50);
}

#[test]
fn bp_pool_try_acquire_fails_when_insufficient_credits() {
    let pool = CreditPool::new(10);
    assert!(!pool.try_acquire(11));
    assert_eq!(pool.available(), 10);
}

#[test]
fn bp_pool_release_replenishes_credits() {
    let pool = CreditPool::new(100);
    assert!(pool.try_acquire(60));
    pool.release(60);
    assert_eq!(pool.available(), 100);
}

#[test]
fn bp_pool_over_release_clamped_to_capacity() {
    let pool = CreditPool::new(50);
    // Release without prior acquire — should not exceed capacity
    pool.release(30);
    assert_eq!(pool.available(), 50);
}

#[test]
fn bp_pool_utilization_zero_when_full() {
    let pool = CreditPool::new(100);
    assert!((pool.utilization() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn bp_pool_utilization_one_when_fully_consumed() {
    let pool = CreditPool::new(100);
    assert!(pool.try_acquire(100));
    assert!((pool.utilization() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn bp_producer_emit_success_consumes_credits() {
    let pool = CreditPool::new(50);
    let mut producer = BackpressureProducer::new(pool);
    let ok = producer
        .try_emit("payload", 20)
        .expect("emit should not error");
    assert!(ok);
    assert_eq!(producer.emitted_total(), 1);
    assert_eq!(producer.pool().available(), 30);
}

#[test]
fn bp_producer_backpressure_when_no_credits() {
    let pool = CreditPool::new(5);
    let mut producer = BackpressureProducer::new(pool);
    producer.try_emit("a", 5).expect("first emit ok");
    let ok = producer
        .try_emit("b", 1)
        .expect("second emit should not error");
    assert!(!ok);
    assert_eq!(producer.dropped_total(), 1);
}

#[test]
fn bp_producer_drain_clears_pending_queue() {
    let pool = CreditPool::new(30);
    let mut producer = BackpressureProducer::new(pool);
    producer.try_emit(1u32, 5).expect("ok");
    producer.try_emit(2u32, 5).expect("ok");
    producer.try_emit(3u32, 5).expect("ok");
    let items: Vec<u32> = producer.drain().map(|p| p.item).collect();
    assert_eq!(items, vec![1, 2, 3]);
    assert_eq!(producer.pending_count(), 0);
}

#[test]
fn bp_consumer_consume_increments_count() {
    let pool = CreditPool::new(100);
    let mut consumer = BackpressureConsumer::new(pool);
    consumer.consume(10);
    consumer.consume(10);
    assert_eq!(consumer.consumed_total(), 2);
}

#[test]
fn bp_consumer_consume_releases_credits_to_pool() {
    let pool = CreditPool::new(100);
    let producer_pool = pool.clone();
    let mut producer = BackpressureProducer::new(producer_pool);
    // drain all credits
    producer.try_emit("x", 100).expect("ok");
    assert_eq!(pool.available(), 0);

    let mut consumer = BackpressureConsumer::new(pool.clone());
    consumer.consume(50);
    assert_eq!(pool.available(), 50);
}

#[test]
fn bp_pool_clone_shares_same_atomic() {
    let pool = CreditPool::new(100);
    let pool2 = pool.clone();
    assert!(pool.try_acquire(30));
    assert_eq!(pool2.available(), 70);
}

#[test]
fn bp_full_producer_consumer_round_trip() {
    let pool = CreditPool::new(20);
    let mut producer = BackpressureProducer::new(pool.clone());
    let mut consumer = BackpressureConsumer::new(pool.clone());

    // Emit 4 items of 5 credits each
    for i in 0u32..4 {
        assert!(producer.try_emit(i, 5).expect("emit ok"));
    }
    assert_eq!(pool.available(), 0);

    // Consumer processes all items, releasing credits
    let items: Vec<_> = producer.drain().collect();
    for item in &items {
        consumer.consume(item.credits_required);
    }
    assert_eq!(pool.available(), 20);
    assert_eq!(consumer.consumed_total(), 4);
}

#[test]
fn bp_invalid_credit_amount_returns_error() {
    let pool = CreditPool::new(100);
    let mut producer = BackpressureProducer::<&str>::new(pool);
    let result = producer.try_emit("x", 0);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// §2  SessionWindow
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn sw_single_session_from_close_events() {
    let cfg = SessionWindowConfig {
        gap_duration: Duration::from_secs(60),
        min_events: 1,
        max_session_duration: None,
    };
    let mut proc = SessionWindowProcessor::new(cfg);
    for i in 0u64..5 {
        proc.process(stream_event(i * 10, i)).expect("process ok");
    }
    proc.flush();
    let sessions = proc.drain_sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].event_count(), 5);
}

#[test]
fn sw_gap_detection_closes_session() {
    let cfg = SessionWindowConfig {
        gap_duration: Duration::from_secs(30),
        min_events: 1,
        max_session_duration: None,
    };
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(0, 0)).expect("ok");
    // 90 s gap > 30 s threshold
    proc.process(stream_event(90, 1)).expect("ok");
    proc.flush();
    let sessions = proc.drain_sessions();
    assert_eq!(sessions.len(), 2);
}

#[test]
fn sw_min_events_filter_drops_small_sessions() {
    let cfg = SessionWindowConfig {
        gap_duration: Duration::from_secs(5),
        min_events: 5,
        max_session_duration: None,
    };
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(0, 0)).expect("ok");
    proc.process(stream_event(1, 1)).expect("ok");
    // only 2 events < min_events=5
    proc.flush();
    let sessions = proc.drain_sessions();
    assert!(sessions.is_empty());
}

#[test]
fn sw_max_session_duration_force_closes() {
    let cfg = SessionWindowConfig {
        gap_duration: Duration::from_secs(1000),
        min_events: 1,
        max_session_duration: Some(Duration::from_secs(40)),
    };
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(0, 0)).expect("ok");
    // 50 s > max_session_duration=40 s
    proc.process(stream_event(50, 1)).expect("ok");
    proc.flush();
    let sessions = proc.drain_sessions();
    assert_eq!(sessions.len(), 2, "force-close should create two sessions");
}

#[test]
fn sw_flush_closes_open_session() {
    let cfg = SessionWindowConfig::default();
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(0, 0)).expect("ok");
    assert_eq!(proc.pending_event_count(), 1);
    proc.flush();
    let sessions = proc.drain_sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(proc.pending_event_count(), 0);
}

#[test]
fn sw_multiple_sessions_from_gapped_stream() {
    let cfg = SessionWindowConfig {
        gap_duration: Duration::from_secs(10),
        min_events: 1,
        max_session_duration: None,
    };
    let mut proc = SessionWindowProcessor::new(cfg);
    // Session 1: t=0,5
    proc.process(stream_event(0, 0)).expect("ok");
    proc.process(stream_event(5, 1)).expect("ok");
    // gap > 10 s
    // Session 2: t=50,55
    proc.process(stream_event(50, 2)).expect("ok");
    proc.process(stream_event(55, 3)).expect("ok");
    // gap > 10 s
    // Session 3: t=200
    proc.process(stream_event(200, 4)).expect("ok");
    proc.flush();
    let sessions = proc.drain_sessions();
    assert_eq!(sessions.len(), 3);
}

#[test]
fn sw_session_id_increments_monotonically() {
    let cfg = SessionWindowConfig {
        gap_duration: Duration::from_secs(5),
        min_events: 1,
        max_session_duration: None,
    };
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(0, 0)).expect("ok");
    proc.process(stream_event(100, 1)).expect("ok"); // gap → close session 0
    proc.flush(); // close session 1
    let sessions = proc.drain_sessions();
    assert_eq!(sessions[0].session_id, 0);
    assert_eq!(sessions[1].session_id, 1);
}

#[test]
fn sw_duration_computation_correct() {
    let cfg = SessionWindowConfig::default();
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(100, 0)).expect("ok");
    proc.process(stream_event(115, 1)).expect("ok");
    proc.flush();
    let sessions = proc.drain_sessions();
    assert_eq!(sessions[0].duration(), Duration::from_secs(15));
}

#[test]
fn sw_empty_processor_yields_no_sessions() {
    let mut proc: SessionWindowProcessor<u64> = SessionWindowProcessor::new(Default::default());
    proc.flush();
    assert_eq!(proc.drain_sessions().len(), 0);
}

#[test]
fn sw_events_within_gap_stay_in_same_session() {
    let cfg = SessionWindowConfig {
        gap_duration: Duration::from_secs(60),
        min_events: 1,
        max_session_duration: None,
    };
    let mut proc = SessionWindowProcessor::new(cfg);
    for i in 0u64..10 {
        // Every 5 s, gap threshold = 60 s — all in one session
        proc.process(stream_event(i * 5, i)).expect("ok");
    }
    proc.flush();
    let sessions = proc.drain_sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].event_count(), 10);
}

#[test]
fn sw_pending_count_resets_after_flush() {
    let cfg = SessionWindowConfig::default();
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(0, 0)).expect("ok");
    proc.process(stream_event(1, 1)).expect("ok");
    assert_eq!(proc.pending_event_count(), 2);
    proc.flush();
    assert_eq!(proc.pending_event_count(), 0);
}

#[test]
fn sw_min_events_one_passes_single_event_sessions() {
    let cfg = SessionWindowConfig {
        gap_duration: Duration::from_secs(5),
        min_events: 1,
        max_session_duration: None,
    };
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(0, 0)).expect("ok");
    proc.flush();
    let sessions = proc.drain_sessions();
    assert_eq!(sessions.len(), 1);
}

#[test]
fn sw_session_is_not_empty_after_events() {
    let cfg = SessionWindowConfig::default();
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(0, 0)).expect("ok");
    proc.flush();
    let sessions = proc.drain_sessions();
    assert!(!sessions[0].is_empty());
}

#[test]
fn sw_total_sessions_closed_tracks_count() {
    let cfg = SessionWindowConfig {
        gap_duration: Duration::from_secs(5),
        min_events: 1,
        max_session_duration: None,
    };
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(0, 0)).expect("ok");
    proc.process(stream_event(100, 1)).expect("ok"); // gap
    proc.flush();
    // 3 sessions were started/closed (incl. empty one at start of second event)
    assert_eq!(proc.total_sessions_closed(), 2);
}

#[test]
fn sw_drain_returns_empty_vec_when_called_twice() {
    let cfg = SessionWindowConfig::default();
    let mut proc = SessionWindowProcessor::new(cfg);
    proc.process(stream_event(0, 0)).expect("ok");
    proc.flush();
    let _first = proc.drain_sessions();
    let second = proc.drain_sessions();
    assert!(second.is_empty());
}

#[test]
fn sw_session_events_are_in_arrival_order() {
    let cfg = SessionWindowConfig::default();
    let mut proc = SessionWindowProcessor::new(cfg);
    for i in [5u64, 2, 8, 1] {
        proc.process(stream_event(i, i)).expect("ok");
    }
    proc.flush();
    let sessions = proc.drain_sessions();
    // Events are stored in arrival order, not sorted by timestamp
    let seqs: Vec<u64> = sessions[0].events.iter().map(|e| e.sequence).collect();
    assert_eq!(seqs, vec![5, 2, 8, 1]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// §3  StreamJoin
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn sj_inner_join_matching_key_and_time() {
    let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
    joiner.add_left(left(100, "k1")).expect("ok");
    joiner.add_right(right(103, "k1")).expect("ok"); // 3 s < 5 s tolerance
    let pairs = joiner.drain_output();
    assert_eq!(pairs.len(), 1);
}

#[test]
fn sj_inner_join_miss_outside_tolerance() {
    let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default()); // 5 s tolerance
    joiner.add_left(left(100, "k1")).expect("ok");
    joiner.add_right(right(120, "k1")).expect("ok"); // 20 s > 5 s
    let pairs = joiner.drain_output();
    assert!(pairs.is_empty());
}

#[test]
fn sj_no_join_on_key_mismatch() {
    let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
    joiner.add_left(left(100, "k1")).expect("ok");
    joiner.add_right(right(100, "k2")).expect("ok");
    let pairs = joiner.drain_output();
    assert!(pairs.is_empty());
}

#[test]
fn sj_left_outer_mode_produces_matching_pair() {
    let cfg = TemporalJoinConfig {
        mode: JoinMode::LeftOuter,
        ..Default::default()
    };
    let mut joiner = TemporalJoiner::new(cfg);
    joiner.add_left(left(100, "k")).expect("ok");
    joiner.add_right(right(102, "k")).expect("ok"); // within 5 s
    let pairs = joiner.drain_output();
    assert_eq!(pairs.len(), 1);
}

#[test]
fn sj_interval_join_matches_within_interval() {
    let cfg = TemporalJoinConfig {
        mode: JoinMode::Interval {
            lower: Duration::from_secs(5),
            upper: Duration::from_secs(15),
        },
        max_buffer_size: 100,
        time_tolerance: Duration::from_secs(1),
    };
    let mut joiner = TemporalJoiner::new(cfg);
    // right.time=100; left must be in [105, 115]
    joiner.add_right(right(100, "k")).expect("ok");
    joiner.add_left(left(110, "k")).expect("ok"); // 110 ∈ [105,115] ✓
    let pairs = joiner.drain_output();
    assert_eq!(pairs.len(), 1);
}

#[test]
fn sj_interval_join_no_match_below_lower() {
    let cfg = TemporalJoinConfig {
        mode: JoinMode::Interval {
            lower: Duration::from_secs(5),
            upper: Duration::from_secs(15),
        },
        max_buffer_size: 100,
        time_tolerance: Duration::from_secs(1),
    };
    let mut joiner = TemporalJoiner::new(cfg);
    joiner.add_right(right(100, "k")).expect("ok");
    joiner.add_left(left(103, "k")).expect("ok"); // 103 < 105 ✗
    let pairs = joiner.drain_output();
    assert!(pairs.is_empty());
}

#[test]
fn sj_buffer_eviction_when_max_buffer_size_exceeded() {
    let cfg = TemporalJoinConfig {
        max_buffer_size: 3,
        ..Default::default()
    };
    let mut joiner = TemporalJoiner::<String, String>::new(cfg);
    for i in 0u64..5 {
        joiner.add_left(left(i * 1000, "k")).expect("ok");
    }
    assert_eq!(joiner.total_expired_left(), 2);
    assert_eq!(joiner.left_buffer_size(), 3);
}

#[test]
fn sj_time_delta_computation_is_correct() {
    let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
    joiner.add_left(left(1000, "k")).expect("ok");
    joiner.add_right(right(1004, "k")).expect("ok");
    let pairs = joiner.drain_output();
    assert_eq!(pairs[0].time_delta, Duration::from_secs(4));
}

#[test]
fn sj_total_joined_counter_increments() {
    let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
    joiner.add_left(left(100, "a")).expect("ok");
    joiner.add_right(right(101, "a")).expect("ok");
    joiner.add_left(left(200, "b")).expect("ok");
    joiner.add_right(right(201, "b")).expect("ok");
    joiner.drain_output();
    assert_eq!(joiner.total_joined(), 2);
}

#[test]
fn sj_add_left_then_right_vs_right_then_left_same_result() {
    let mut j1 = TemporalJoiner::new(TemporalJoinConfig::default());
    j1.add_left(left(100, "k")).expect("ok");
    j1.add_right(right(102, "k")).expect("ok");
    let p1 = j1.drain_output();

    let mut j2 = TemporalJoiner::new(TemporalJoinConfig::default());
    j2.add_right(right(102, "k")).expect("ok");
    j2.add_left(left(100, "k")).expect("ok");
    let p2 = j2.drain_output();

    assert_eq!(p1.len(), 1);
    assert_eq!(p2.len(), 1);
    assert_eq!(p1[0].time_delta, p2[0].time_delta);
}

#[test]
fn sj_multiple_right_events_match_single_left() {
    let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
    joiner.add_right(right(100, "k")).expect("ok");
    joiner.add_right(right(101, "k")).expect("ok");
    joiner.add_left(left(102, "k")).expect("ok"); // matches both right events
    let pairs = joiner.drain_output();
    assert_eq!(pairs.len(), 2);
}

#[test]
fn sj_no_cross_key_contamination() {
    let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
    joiner.add_left(left(100, "alpha")).expect("ok");
    joiner.add_left(left(100, "beta")).expect("ok");
    joiner.add_right(right(101, "alpha")).expect("ok");
    let pairs = joiner.drain_output();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].left.key, "alpha");
}

#[test]
fn sj_expired_right_counter_increments_on_eviction() {
    let cfg = TemporalJoinConfig {
        max_buffer_size: 2,
        ..Default::default()
    };
    let mut joiner = TemporalJoiner::<String, String>::new(cfg);
    joiner.add_right(right(0, "a")).expect("ok");
    joiner.add_right(right(1, "a")).expect("ok");
    joiner.add_right(right(2, "a")).expect("ok"); // evicts first
    assert_eq!(joiner.total_expired_right(), 1);
}

#[test]
fn sj_drain_output_clears_queue() {
    let mut joiner = TemporalJoiner::new(TemporalJoinConfig::default());
    joiner.add_left(left(100, "k")).expect("ok");
    joiner.add_right(right(101, "k")).expect("ok");
    let first_drain = joiner.drain_output();
    assert_eq!(first_drain.len(), 1);
    let second_drain = joiner.drain_output();
    assert!(second_drain.is_empty());
}

#[test]
fn sj_exact_tolerance_boundary_matches() {
    let cfg = TemporalJoinConfig {
        time_tolerance: Duration::from_secs(10),
        ..Default::default()
    };
    let mut joiner = TemporalJoiner::new(cfg);
    joiner.add_left(left(100, "k")).expect("ok");
    // Exactly at the tolerance boundary: 10 s ≤ 10 s → should match
    joiner.add_right(right(110, "k")).expect("ok");
    let pairs = joiner.drain_output();
    assert_eq!(pairs.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// §4  Checkpoint
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ck_state_serialize_deserialize_round_trip_empty() {
    let id = CheckpointId::new("stream-a", 100);
    let mut state = CheckpointState::new(id);
    state.watermark_ns = 123_456_789;
    state.event_count = 100;
    let bytes = state.serialize();
    let decoded = CheckpointState::deserialize("stream-a", &bytes).expect("should succeed");
    assert_eq!(decoded.id.sequence, 100);
    assert_eq!(decoded.watermark_ns, 123_456_789);
    assert_eq!(decoded.event_count, 100);
}

#[test]
fn ck_deserialize_truncated_data_returns_error() {
    let result = CheckpointState::deserialize("s", &[0u8; 10]);
    assert!(result.is_err());
}

#[test]
fn ck_deserialize_empty_slice_returns_error() {
    let result = CheckpointState::deserialize("s", &[]);
    assert!(result.is_err());
}

#[test]
fn ck_store_save_and_latest() {
    let mut store = InMemoryCheckpointStore::new(5);
    let id = CheckpointId::new("s", 42);
    store.save(CheckpointState::new(id)).expect("save ok");
    let latest = store.latest("s").expect("should have latest");
    assert_eq!(latest.id.sequence, 42);
}

#[test]
fn ck_store_trims_to_max_per_stream() {
    let mut store = InMemoryCheckpointStore::new(3);
    for i in 0u64..6 {
        store
            .save(CheckpointState::new(CheckpointId::new("s", i)))
            .expect("save ok");
    }
    assert_eq!(store.checkpoint_count("s"), 3);
    // Latest should be the most recent (seq=5)
    assert_eq!(store.latest("s").unwrap().id.sequence, 5);
}

#[test]
fn ck_delete_before_removes_old_checkpoints() {
    let mut store = InMemoryCheckpointStore::new(10);
    for i in [0u64, 10, 20, 30, 40] {
        store
            .save(CheckpointState::new(CheckpointId::new("s", i)))
            .expect("save ok");
    }
    store.delete_before("s", 20);
    let remaining = store.list("s");
    assert!(remaining.iter().all(|c| c.id.sequence >= 20));
    assert_eq!(remaining.len(), 3); // seq 20, 30, 40
}

#[test]
fn ck_manager_triggers_checkpoint_at_interval() {
    let store = InMemoryCheckpointStore::new(20);
    let mut mgr = CheckpointManager::new(store, 100);
    // Events 0-99: no checkpoint
    for seq in 0u64..100 {
        let triggered = mgr.on_event("s", seq, 0).expect("ok");
        assert!(!triggered);
    }
    // Event 100: checkpoint fires
    let triggered = mgr.on_event("s", 100, 0).expect("ok");
    assert!(triggered);
    assert_eq!(mgr.total_checkpoints(), 1);
}

#[test]
fn ck_manager_recover_returns_last_sequence() {
    let store = InMemoryCheckpointStore::new(10);
    let mut mgr = CheckpointManager::new(store, 50);
    mgr.on_event("s", 50, 0).expect("ok");
    mgr.on_event("s", 100, 0).expect("ok");
    assert_eq!(mgr.recover("s"), Some(100));
}

#[test]
fn ck_total_checkpoints_counter_correct() {
    let store = InMemoryCheckpointStore::new(10);
    let mut mgr = CheckpointManager::new(store, 10);
    // Events 0-49 → checkpoints at 10,20,30,40
    for seq in 0u64..=40 {
        mgr.on_event("s", seq, 0).expect("ok");
    }
    assert_eq!(mgr.total_checkpoints(), 4);
}

#[test]
fn ck_multiple_streams_independent() {
    let mut store = InMemoryCheckpointStore::new(5);
    for seq in [10u64, 20, 30] {
        store
            .save(CheckpointState::new(CheckpointId::new("stream-a", seq)))
            .expect("ok");
        store
            .save(CheckpointState::new(CheckpointId::new(
                "stream-b",
                seq * 10,
            )))
            .expect("ok");
    }
    assert_eq!(store.checkpoint_count("stream-a"), 3);
    assert_eq!(store.checkpoint_count("stream-b"), 3);
    assert_eq!(store.latest("stream-a").unwrap().id.sequence, 30);
    assert_eq!(store.latest("stream-b").unwrap().id.sequence, 300);
}

#[test]
fn ck_operator_states_round_trip() {
    let id = CheckpointId::new("s", 1);
    let mut state = CheckpointState::new(id);
    state.set_operator_state("op_a", vec![10, 20, 30]);
    state.set_operator_state("op_b", vec![99]);
    let bytes = state.serialize();
    let decoded = CheckpointState::deserialize("s", &bytes).expect("ok");
    assert_eq!(decoded.operator_states.get("op_a"), Some(&vec![10, 20, 30]));
    assert_eq!(decoded.operator_states.get("op_b"), Some(&vec![99]));
}

#[test]
fn ck_source_offsets_round_trip() {
    let id = CheckpointId::new("s", 2);
    let mut state = CheckpointState::new(id);
    state.set_source_offset("src-0", 8192);
    state.set_source_offset("src-1", 16384);
    let bytes = state.serialize();
    let decoded = CheckpointState::deserialize("s", &bytes).expect("ok");
    assert_eq!(decoded.source_offsets.get("src-0"), Some(&8192u64));
    assert_eq!(decoded.source_offsets.get("src-1"), Some(&16384u64));
}

#[test]
fn ck_recover_returns_none_before_first_checkpoint() {
    let store = InMemoryCheckpointStore::new(5);
    let mgr = CheckpointManager::new(store, 100);
    assert!(mgr.recover("s").is_none());
}

#[test]
fn ck_store_latest_returns_none_for_unknown_stream() {
    let store = InMemoryCheckpointStore::new(5);
    assert!(store.latest("ghost-stream").is_none());
}

#[test]
fn ck_watermark_preserved_in_checkpoint() {
    let store = InMemoryCheckpointStore::new(10);
    let mut mgr = CheckpointManager::new(store, 10);
    mgr.on_event("s", 10, 9_999_999).expect("ok");
    let seq = mgr.recover("s").expect("should have checkpoint");
    assert_eq!(seq, 10);
    let latest = mgr.store().latest("s").expect("should exist");
    assert_eq!(latest.watermark_ns, 9_999_999);
}
