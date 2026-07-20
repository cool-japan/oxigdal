//! Integration tests for the codec chaining pipeline (`CodecPipeline`).

use oxigeo_compress::codecs::{CodecPipeline, PipelineStage};

/// A repetitive byte payload that compresses well and exercises shuffle tails.
fn repetitive_payload() -> Vec<u8> {
    let mut data = Vec::with_capacity(4096);
    for i in 0..4096u32 {
        data.push((i % 7) as u8);
    }
    data
}

/// An `f32` array serialised to little-endian bytes (length divisible by 4).
fn f32_array_bytes() -> Vec<u8> {
    let mut data = Vec::with_capacity(1024 * 4);
    for i in 0..1024u32 {
        let value = (i as f32) * 0.25 + 100.0;
        data.extend_from_slice(&value.to_le_bytes());
    }
    data
}

#[test]
fn test_pipeline_empty_round_trip() {
    let pipeline = CodecPipeline::new();
    let data = b"empty pipeline payload".to_vec();

    let compressed = pipeline.compress(&data).expect("compress must succeed");
    // Empty pipeline emits a header-only frame: magic(4) + version(1) +
    // num_stages(1) followed by the verbatim payload.
    assert_eq!(compressed.len(), 6 + data.len());

    let restored = pipeline.decompress(&compressed).expect("decompress");
    assert_eq!(restored, data);

    let self_described =
        CodecPipeline::decompress_self_describing(&compressed).expect("self-describe");
    assert_eq!(self_described, data);
}

#[test]
fn test_pipeline_single_stage_zstd_round_trip() {
    let pipeline = CodecPipeline::new().push(PipelineStage::Zstd);
    let data = repetitive_payload();

    let compressed = pipeline.compress(&data).expect("compress");
    let restored = pipeline.decompress(&compressed).expect("decompress");
    assert_eq!(restored, data);
}

#[test]
fn test_pipeline_shuffle_zstd_round_trip() {
    let pipeline = CodecPipeline::new()
        .push(PipelineStage::Shuffle { typesize: 4 })
        .push(PipelineStage::Zstd);
    let data = f32_array_bytes();

    let compressed = pipeline.compress(&data).expect("compress");
    let restored = pipeline.decompress(&compressed).expect("decompress");
    assert_eq!(restored, data);
}

#[test]
fn test_pipeline_three_stages_shuffle_delta_zstd() {
    let pipeline = CodecPipeline::new()
        .push(PipelineStage::Shuffle { typesize: 4 })
        .push(PipelineStage::Delta)
        .push(PipelineStage::Zstd);
    // Length divisible by 4 so the i32-based Delta stage consumes every byte.
    let data = f32_array_bytes();

    let compressed = pipeline.compress(&data).expect("compress");
    let restored = pipeline.decompress(&compressed).expect("decompress");
    assert_eq!(restored, data);

    let self_described =
        CodecPipeline::decompress_self_describing(&compressed).expect("self-describe");
    assert_eq!(self_described, data);
}

#[test]
fn test_pipeline_frame_header_magic_and_version() {
    let pipeline = CodecPipeline::new()
        .push(PipelineStage::Shuffle { typesize: 8 })
        .push(PipelineStage::Lz4);
    let data = repetitive_payload();

    let compressed = pipeline.compress(&data).expect("compress");

    // Magic 'OXPL'.
    assert_eq!(&compressed[0..4], b"OXPL");
    // Version byte == 1.
    assert_eq!(compressed[4], 1);
    // num_stages == 2.
    assert_eq!(compressed[5], 2);
    // Stage 0: Shuffle (id 1) with typesize 8.
    assert_eq!(compressed[6], 1);
    assert_eq!(compressed[7], 8);
    // Stage 1: Lz4 (id 4) with typesize 0.
    assert_eq!(compressed[8], 4);
    assert_eq!(compressed[9], 0);
}

#[test]
fn test_pipeline_decompress_self_describing_reconstructs_stages() {
    // Shuffle(2) -> Delta -> Deflate exercises a non-shuffle entropy stage
    // distinct from the other tests' Zstd/Lz4 chains.
    let producer = CodecPipeline::new()
        .push(PipelineStage::Shuffle { typesize: 2 })
        .push(PipelineStage::Delta)
        .push(PipelineStage::Deflate);
    let data = f32_array_bytes();

    let compressed = producer.compress(&data).expect("compress");

    // A caller that knows nothing about the producing pipeline can still
    // decompress purely from the frame header.
    let restored = CodecPipeline::decompress_self_describing(&compressed).expect("self-describe");
    assert_eq!(restored, data);

    // The reconstructed stage list (recoverable via a fresh pipeline's
    // `decompress`) matches because the header is self-describing.
    let blank = CodecPipeline::new();
    let via_blank = blank
        .decompress(&compressed)
        .expect("header-driven decompress");
    assert_eq!(via_blank, data);
}

#[test]
fn test_pipeline_decompress_truncated_header_errors() {
    // Fewer than the 6-byte fixed prefix.
    let truncated = vec![b'O', b'X', b'P'];
    assert!(CodecPipeline::decompress_self_describing(&truncated).is_err());

    // Valid prefix declaring 3 stages but missing the stage descriptors.
    let mut short = vec![b'O', b'X', b'P', b'L', 1, 3];
    short.push(4); // partial — needs 6 descriptor bytes, only 1 supplied
    assert!(CodecPipeline::decompress_self_describing(&short).is_err());
}

#[test]
fn test_pipeline_decompress_bad_magic_errors() {
    // Correct length, wrong magic.
    let bad_magic = vec![b'X', b'X', b'X', b'X', 1, 0];
    assert!(CodecPipeline::decompress_self_describing(&bad_magic).is_err());

    let pipeline = CodecPipeline::new();
    assert!(pipeline.decompress(&bad_magic).is_err());
}

#[test]
fn test_pipeline_decompress_unknown_stage_id_errors() {
    // Header declares one stage with id 99 (no such codec).
    let unknown_stage = vec![b'O', b'X', b'P', b'L', 1, 1, 99, 0];
    assert!(CodecPipeline::decompress_self_describing(&unknown_stage).is_err());

    let pipeline = CodecPipeline::new();
    assert!(pipeline.decompress(&unknown_stage).is_err());

    // Also reject an unsupported version byte.
    let bad_version = vec![b'O', b'X', b'P', b'L', 99, 0];
    assert!(CodecPipeline::decompress_self_describing(&bad_version).is_err());
}

#[test]
fn test_pipeline_round_trip_f32_array_shuffle_lz4() {
    let pipeline = CodecPipeline::new()
        .push(PipelineStage::Shuffle { typesize: 4 })
        .push(PipelineStage::Lz4);
    let data = f32_array_bytes();

    let compressed = pipeline.compress(&data).expect("compress");
    let restored = pipeline.decompress(&compressed).expect("decompress");
    assert_eq!(restored, data);

    // Reinterpret the restored bytes as f32 and confirm exact values.
    assert_eq!(restored.len() % 4, 0);
    for (index, chunk) in restored.chunks_exact(4).enumerate() {
        let bytes: [u8; 4] = chunk.try_into().expect("4-byte chunk");
        let value = f32::from_le_bytes(bytes);
        let expected = (index as f32) * 0.25 + 100.0;
        assert_eq!(value, expected);
    }
}

#[test]
fn test_pipeline_builder_push_chains() {
    // `push` is chainable and preserves stage order.
    let pipeline = CodecPipeline::new()
        .push(PipelineStage::Shuffle { typesize: 4 })
        .push(PipelineStage::Delta)
        .push(PipelineStage::Rle)
        .push(PipelineStage::Zstd);

    let stages = pipeline.stages();
    assert_eq!(stages.len(), 4);
    assert_eq!(stages[0], PipelineStage::Shuffle { typesize: 4 });
    assert_eq!(stages[1], PipelineStage::Delta);
    assert_eq!(stages[2], PipelineStage::Rle);
    assert_eq!(stages[3], PipelineStage::Zstd);

    // An empty builder has no stages.
    assert!(CodecPipeline::new().stages().is_empty());
}

#[test]
fn test_pipeline_smaller_than_raw_for_repetitive_input() {
    let data = vec![b'a'; 4096];

    let pipeline = CodecPipeline::new()
        .push(PipelineStage::Rle)
        .push(PipelineStage::Zstd);

    let compressed = pipeline.compress(&data).expect("compress");
    // Even counting the frame header, a 4 KiB run of one byte must shrink.
    assert!(
        compressed.len() < data.len(),
        "compressed {} bytes should be smaller than raw {} bytes",
        compressed.len(),
        data.len()
    );

    let restored = pipeline.decompress(&compressed).expect("decompress");
    assert_eq!(restored, data);
}
