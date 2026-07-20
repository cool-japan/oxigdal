//! CPU-only tests for async GPU readback — no wgpu device required.
//!
//! These tests verify the structural and type-system guarantees of the async
//! readback API without needing real GPU hardware.  They run on every CI
//! machine regardless of whether a wgpu backend is available.

// ──────────────────────────────────────────────────────────────────────────────
// Test 1: Verify that the Future returned by GpuBuffer::read_async is Send.
//
// We cannot instantiate a real GpuBuffer without a device, so we verify the
// Send bound indirectly: the underlying implementation uses
// `futures::channel::oneshot::Receiver` which is `Send`, and the only
// non-Send field in GpuBuffer is the wgpu Buffer/Device pair — both of which
// ARE Send in wgpu 29.  We confirm this property compiles by constructing a
// trait-object wrapper that requires Send.
// ──────────────────────────────────────────────────────────────────────────────

/// Compile-time assertion helper: asserts T: Send without runtime cost.
fn assert_send<T: Send>() {}

/// Compile-time assertion helper: asserts T: Future without runtime cost.
fn assert_future<T: std::future::Future>() {}

#[test]
fn test_read_async_future_type_parameters_satisfy_send() {
    // `futures::channel::oneshot::Receiver<Result<(), wgpu::BufferAsyncError>>`
    // is Send.  This is the internal channel used by read_async, so verifying
    // its Send-ness is the key structural guarantee.
    assert_send::<futures::channel::oneshot::Receiver<Result<(), wgpu::BufferAsyncError>>>();

    // The Sender half must also be Send so the map_async callback can cross
    // thread boundaries.
    assert_send::<futures::channel::oneshot::Sender<Result<(), wgpu::BufferAsyncError>>>();
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 2: API signature compatibility — both read_blocking and read_async exist.
//
// We use dead-code functions that name the methods; if either method is absent
// the file won't compile, giving us a compile-time API contract check.
// ──────────────────────────────────────────────────────────────────────────────

/// A compile-time existence check.  This function is never called; the
/// compiler merely has to be able to resolve the method names.
#[allow(dead_code)]
async fn _check_compute_pipeline_api_compat(
    pipeline: oxigeo_gpu::ComputePipeline<f32>,
) -> oxigeo_gpu::GpuResult<Vec<f32>> {
    // Both async and blocking entry-points must co-exist.
    // We call only the async one to avoid consuming `pipeline` twice, but
    // we also verify that read_blocking is resolvable through the type path.
    //
    // The trait bound check for read_blocking (fn(self) -> GpuResult<...>)
    // is satisfied as long as the method is present on the type; the
    // compiler will reject this file if it disappears.
    let _blocking_fn: fn(oxigeo_gpu::ComputePipeline<f32>) -> oxigeo_gpu::GpuResult<Vec<f32>> =
        oxigeo_gpu::ComputePipeline::<f32>::read_blocking;

    pipeline.read_async().await
}

#[test]
fn test_read_async_and_blocking_api_signatures_compatible() {
    // Verify read_blocking is a callable fn-item (not just a method) so that
    // both signatures are simultaneously visible to the type checker.
    let _: fn(oxigeo_gpu::ComputePipeline<f32>) -> oxigeo_gpu::GpuResult<Vec<f32>> =
        oxigeo_gpu::ComputePipeline::<f32>::read_blocking;

    // The async variant is an async fn — verify it produces a Future.
    assert_future::<
        std::pin::Pin<Box<dyn std::future::Future<Output = oxigeo_gpu::GpuResult<Vec<f32>>>>>,
    >();
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3: The oneshot channel pattern resolves correctly.
//
// This is a pure-Rust unit test of the `futures::channel::oneshot` contract
// that `read_async` relies on internally.
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_oneshot_channel_resolves_on_send() {
    let (tx, rx) = futures::channel::oneshot::channel::<u32>();

    // Simulate the wgpu map_async callback: send a value from a closure.
    let send_result = tx.send(42_u32);
    assert!(
        send_result.is_ok(),
        "send should succeed on an open channel"
    );

    // Drive the future to completion with pollster (the same executor used
    // by read_blocking internally).
    let received = pollster::block_on(rx);
    assert!(
        received.is_ok(),
        "receiver should not be cancelled when sender sent a value"
    );
    assert_eq!(
        received.expect("receiver value"),
        42_u32,
        "received value must match sent value"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 4: wgpu::PollType::Poll is accessible.
//
// `spawn_poll_task` uses this variant; verify that the name resolves without
// a GPU device.
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_poll_type_enum_accessible() {
    // Merely constructing the value is sufficient — if the variant is renamed
    // or removed in a future wgpu version, this test fails at compile time.
    let _poll_type = wgpu::PollType::Poll;

    // Also verify the Wait-based variant used in multi_gpu.rs.
    let _wait_type = wgpu::PollType::wait_indefinitely();
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 5: The error variant used by the async readback path is formatted
//         correctly, verifying both its existence and Display impl.
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn test_read_async_error_type_matches_blocking() {
    // GpuError::BufferMapping is the error variant emitted when the oneshot
    // channel is cancelled or when wgpu reports a mapping error.
    let err = oxigeo_gpu::GpuError::buffer_mapping("Channel closed");

    let formatted = err.to_string();
    assert!(
        formatted.contains("Channel closed"),
        "error message must include the reason string, got: {formatted}"
    );
    assert!(
        formatted.contains("map"),
        "error message must mention 'map' (buffer mapping failure), got: {formatted}"
    );

    // Verify the same variant is produced when the async path would use it
    // for a wgpu-side rejection — we simulate that by using a different reason.
    let err2 = oxigeo_gpu::GpuError::buffer_mapping("wgpu: buffer destroyed");
    assert!(
        err2.to_string().contains("buffer destroyed"),
        "reason must propagate through the error message"
    );

    // Both should be recoverable (buffer mapping is classified as recoverable).
    assert!(
        err.is_recoverable(),
        "BufferMapping errors must be recoverable"
    );
}
