//! Message framing for WebSocket protocol

use crate::error::{Error, Result};
use bytes::{BufMut, Bytes, BytesMut};

/// Frame type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType {
    /// Data frame
    Data = 0,
    /// Control frame
    Control = 1,
    /// Heartbeat frame
    Heartbeat = 2,
    /// Fragment start
    FragmentStart = 3,
    /// Fragment continuation
    FragmentContinuation = 4,
    /// Fragment end
    FragmentEnd = 5,
}

impl TryFrom<u8> for FrameType {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(FrameType::Data),
            1 => Ok(FrameType::Control),
            2 => Ok(FrameType::Heartbeat),
            3 => Ok(FrameType::FragmentStart),
            4 => Ok(FrameType::FragmentContinuation),
            5 => Ok(FrameType::FragmentEnd),
            _ => Err(Error::Protocol(format!("Invalid frame type: {}", value))),
        }
    }
}

/// Frame header structure
///
/// Layout (8 bytes):
/// - Byte 0: Frame type (4 bits) | Protocol version (4 bits)
/// - Byte 1: Flags (compressed: 1 bit, fragmented: 1 bit, reserved: 6 bits)
/// - Bytes 2-5: Payload length (u32, big-endian)
/// - Bytes 6-7: Reserved
#[derive(Debug, Clone)]
pub struct FrameHeader {
    /// Frame type
    pub frame_type: FrameType,
    /// Protocol version
    pub version: u8,
    /// Compressed flag
    pub compressed: bool,
    /// Fragmented flag
    pub fragmented: bool,
    /// Payload length
    pub payload_length: u32,
}

impl FrameHeader {
    /// Header size in bytes
    pub const SIZE: usize = 8;

    /// Create a new frame header
    pub fn new(frame_type: FrameType, version: u8, compressed: bool, payload_length: u32) -> Self {
        Self {
            frame_type,
            version,
            compressed,
            fragmented: false,
            payload_length,
        }
    }

    /// Encode frame header to bytes
    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut buf = [0u8; Self::SIZE];

        // Byte 0: frame type (upper 4 bits) | version (lower 4 bits)
        buf[0] = ((self.frame_type as u8) << 4) | (self.version & 0x0F);

        // Byte 1: flags
        let mut flags = 0u8;
        if self.compressed {
            flags |= 0x80; // Set bit 7
        }
        if self.fragmented {
            flags |= 0x40; // Set bit 6
        }
        buf[1] = flags;

        // Bytes 2-5: payload length (big-endian)
        buf[2..6].copy_from_slice(&self.payload_length.to_be_bytes());

        // Bytes 6-7: reserved (zeros)
        buf
    }

    /// Decode frame header from bytes
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < Self::SIZE {
            return Err(Error::Protocol(format!(
                "Insufficient data for frame header: expected {}, got {}",
                Self::SIZE,
                data.len()
            )));
        }

        // Parse byte 0
        let frame_type = FrameType::try_from(data[0] >> 4)?;
        let version = data[0] & 0x0F;

        // Parse byte 1
        let compressed = (data[1] & 0x80) != 0;
        let fragmented = (data[1] & 0x40) != 0;

        // Parse bytes 2-5
        let payload_length = u32::from_be_bytes([data[2], data[3], data[4], data[5]]);

        Ok(Self {
            frame_type,
            version,
            compressed,
            fragmented,
            payload_length,
        })
    }

    /// Get total frame size (header + payload)
    pub fn total_size(&self) -> usize {
        Self::SIZE + self.payload_length as usize
    }
}

/// WebSocket frame
#[derive(Debug, Clone)]
pub struct Frame {
    /// Frame header
    pub header: FrameHeader,
    /// Frame payload
    pub payload: Bytes,
}

impl Frame {
    /// Create a new frame
    pub fn new(frame_type: FrameType, version: u8, compressed: bool, payload: Bytes) -> Self {
        let header = FrameHeader::new(frame_type, version, compressed, payload.len() as u32);
        Self { header, payload }
    }

    /// Create a data frame
    pub fn data(version: u8, compressed: bool, payload: Bytes) -> Self {
        Self::new(FrameType::Data, version, compressed, payload)
    }

    /// Create a control frame
    pub fn control(version: u8, payload: Bytes) -> Self {
        Self::new(FrameType::Control, version, false, payload)
    }

    /// Create a heartbeat frame
    pub fn heartbeat(version: u8) -> Self {
        Self::new(FrameType::Heartbeat, version, false, Bytes::new())
    }

    /// Get frame size
    pub fn size(&self) -> usize {
        self.header.total_size()
    }
}

/// Frame codec for encoding and decoding frames
pub struct FrameCodec {
    max_payload_size: u32,
}

impl FrameCodec {
    /// Create a new frame codec
    pub fn new() -> Self {
        Self {
            max_payload_size: 16 * 1024 * 1024, // 16MB
        }
    }

    /// Create a new frame codec with custom max payload size
    pub fn with_max_payload_size(max_payload_size: u32) -> Self {
        Self { max_payload_size }
    }

    /// Encode a frame to bytes
    pub fn encode(&self, frame: &Frame) -> Result<Bytes> {
        if frame.header.payload_length > self.max_payload_size {
            return Err(Error::Protocol(format!(
                "Payload size {} exceeds maximum {}",
                frame.header.payload_length, self.max_payload_size
            )));
        }

        let mut buf = BytesMut::with_capacity(frame.size());

        // Write header
        buf.put_slice(&frame.header.encode());

        // Write payload
        buf.put_slice(&frame.payload);

        Ok(buf.freeze())
    }

    /// Decode a frame from bytes
    pub fn decode(&self, data: &[u8]) -> Result<Frame> {
        // Parse header
        let header = FrameHeader::decode(data)?;

        // Validate payload size
        if header.payload_length > self.max_payload_size {
            return Err(Error::Protocol(format!(
                "Payload size {} exceeds maximum {}",
                header.payload_length, self.max_payload_size
            )));
        }

        // Check total data length
        let total_size = header.total_size();
        if data.len() < total_size {
            return Err(Error::Protocol(format!(
                "Insufficient data for frame: expected {}, got {}",
                total_size,
                data.len()
            )));
        }

        // Extract payload
        let payload = Bytes::copy_from_slice(&data[FrameHeader::SIZE..total_size]);

        Ok(Frame { header, payload })
    }

    /// Decode multiple frames from a buffer
    pub fn decode_all(&self, data: &[u8]) -> Result<Vec<Frame>> {
        let mut frames = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            if data.len() - offset < FrameHeader::SIZE {
                break;
            }

            let header = FrameHeader::decode(&data[offset..])?;
            let total_size = header.total_size();

            if data.len() - offset < total_size {
                break;
            }

            let frame = self.decode(&data[offset..])?;
            frames.push(frame);

            offset += total_size;
        }

        Ok(frames)
    }
}

impl Default for FrameCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_header_encode_decode() -> Result<()> {
        let header = FrameHeader::new(FrameType::Data, 1, true, 1024);
        let encoded = header.encode();
        let decoded = FrameHeader::decode(&encoded)?;

        assert_eq!(header.frame_type as u8, decoded.frame_type as u8);
        assert_eq!(header.version, decoded.version);
        assert_eq!(header.compressed, decoded.compressed);
        assert_eq!(header.payload_length, decoded.payload_length);
        Ok(())
    }

    #[test]
    fn test_frame_encode_decode() -> Result<()> {
        let codec = FrameCodec::new();
        let payload = Bytes::from(vec![1, 2, 3, 4, 5]);
        let frame = Frame::data(1, false, payload.clone());

        let encoded = codec.encode(&frame)?;
        let decoded = codec.decode(&encoded)?;

        assert_eq!(
            frame.header.frame_type as u8,
            decoded.header.frame_type as u8
        );
        assert_eq!(frame.payload, decoded.payload);
        Ok(())
    }

    #[test]
    fn test_frame_codec_decode_all() -> Result<()> {
        let codec = FrameCodec::new();

        // Create multiple frames
        let frame1 = Frame::data(1, false, Bytes::from(vec![1, 2, 3]));
        let frame2 = Frame::data(1, false, Bytes::from(vec![4, 5, 6]));

        // Encode them
        let mut buf = BytesMut::new();
        buf.put_slice(&codec.encode(&frame1)?);
        buf.put_slice(&codec.encode(&frame2)?);

        // Decode all
        let frames = codec.decode_all(&buf)?;

        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].payload, Bytes::from(vec![1, 2, 3]));
        assert_eq!(frames[1].payload, Bytes::from(vec![4, 5, 6]));
        Ok(())
    }

    #[test]
    fn test_frame_heartbeat() -> Result<()> {
        let codec = FrameCodec::new();
        let frame = Frame::heartbeat(1);

        let encoded = codec.encode(&frame)?;
        let decoded = codec.decode(&encoded)?;

        assert_eq!(decoded.header.frame_type as u8, FrameType::Heartbeat as u8);
        assert!(decoded.payload.is_empty());
        Ok(())
    }

    #[test]
    fn test_frame_max_size() {
        let codec = FrameCodec::with_max_payload_size(100);
        let payload = Bytes::from(vec![0; 200]);
        let frame = Frame::data(1, false, payload);

        let result = codec.encode(&frame);
        assert!(result.is_err());
    }
}
