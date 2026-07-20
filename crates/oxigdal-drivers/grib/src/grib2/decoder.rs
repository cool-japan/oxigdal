//! GRIB2 data decoder

use crate::error::{GribError, Result};
use crate::grib2::Grib2Message;

/// GRIB2 data decoder for unpacking binary meteorological data.
pub struct Grib2Decoder<'a> {
    /// Reference to the GRIB2 message to decode
    message: &'a Grib2Message,
}

impl<'a> Grib2Decoder<'a> {
    /// Creates a new decoder for the given GRIB2 message.
    pub fn new(message: &'a Grib2Message) -> Result<Self> {
        Ok(Self { message })
    }

    /// Decodes the packed binary data into f32 values.
    pub fn decode(&self) -> Result<Vec<f32>> {
        let dr = &self.message.data_representation;
        let num_points = self.message.num_points();

        // Dispatch: DRT 5.3 (complex packing + spatial differencing) takes
        // precedence over DRT 5.2 because a 5.3 section also carries the
        // complex-packing parameter block.
        if let (Some(cp), Some(sd)) = (&dr.complex_packing, &dr.spatial_diff) {
            let raw = decode_complex_with_spatial_diff(
                &self.message.data_section.packed_data,
                cp,
                sd,
                num_points,
            )?;
            return Ok(self.apply_bitmap(raw, num_points));
        }

        // Dispatch: DRT 5.2 (complex packing without spatial differencing).
        if let Some(cp) = &dr.complex_packing {
            let raw =
                decode_complex_packing(&self.message.data_section.packed_data, cp, num_points)?;
            return Ok(self.apply_bitmap(raw, num_points));
        }

        // DRT 5.0 / 5.40: simple packing.
        if dr.bits_per_value == 0 {
            return Ok(vec![dr.reference_value; num_points]);
        }

        let packed_values = self.unpack_bits(
            &self.message.data_section.packed_data,
            dr.bits_per_value,
            dr.num_data_points as usize,
        )?;

        let scale = dr.scale_multiplier();
        let decimal = dr.decimal_divisor();

        let mut values = Vec::with_capacity(num_points);

        if let Some(bitmap) = &self.message.bitmap {
            let mut packed_idx = 0;
            for &present in bitmap.iter().take(num_points) {
                if present {
                    if packed_idx >= packed_values.len() {
                        return Err(GribError::DecodingError(
                            "Packed data too short for bitmap".to_string(),
                        ));
                    }
                    let value =
                        (dr.reference_value + packed_values[packed_idx] as f32 * scale) / decimal;
                    values.push(value);
                    packed_idx += 1;
                } else {
                    values.push(f32::NAN);
                }
            }
        } else {
            for &raw in packed_values.iter().take(num_points) {
                let value = (dr.reference_value + raw as f32 * scale) / decimal;
                values.push(value);
            }
        }

        Ok(values)
    }

    fn unpack_bits(&self, data: &[u8], num_bits: u8, num_values: usize) -> Result<Vec<u32>> {
        if num_bits > 32 {
            return Err(GribError::InvalidBitOperation(format!(
                "Number of bits {} exceeds 32",
                num_bits
            )));
        }

        let mut values = Vec::with_capacity(num_values);
        let mut bit_offset = 0usize;

        for _ in 0..num_values {
            let value = self.read_bits(data, bit_offset, num_bits as usize)?;
            values.push(value);
            bit_offset += num_bits as usize;
        }

        Ok(values)
    }

    fn read_bits(&self, data: &[u8], bit_offset: usize, num_bits: usize) -> Result<u32> {
        if num_bits == 0 {
            return Ok(0);
        }

        let byte_offset = bit_offset / 8;
        let bit_in_byte = bit_offset % 8;
        let bytes_needed = (bit_in_byte + num_bits).div_ceil(8).max(1);

        if byte_offset + bytes_needed > data.len() {
            return Err(GribError::TruncatedMessage {
                expected: byte_offset + bytes_needed,
                actual: data.len(),
            });
        }

        let mut accumulator = 0u64;
        for i in 0..bytes_needed.min(8) {
            if byte_offset + i < data.len() {
                accumulator = (accumulator << 8) | (data[byte_offset + i] as u64);
            }
        }

        let total_bits = bytes_needed * 8;
        let shift_amount = total_bits - bit_in_byte - num_bits;
        let value = (accumulator >> shift_amount) & ((1u64 << num_bits) - 1);

        Ok(value as u32)
    }

    /// Maps a decoded value array onto the grid, honouring the optional
    /// Section 6 bitmap. When a bitmap is present, decoded values fill the
    /// positions flagged present (in order) and absent positions become NaN.
    /// When no bitmap is present the array is returned (truncated/padded to
    /// `num_points`) unchanged.
    fn apply_bitmap(&self, decoded: Vec<f32>, num_points: usize) -> Vec<f32> {
        match &self.message.bitmap {
            Some(bitmap) => {
                let mut values = Vec::with_capacity(num_points);
                let mut idx = 0usize;
                for &present in bitmap.iter().take(num_points) {
                    if present {
                        match decoded.get(idx) {
                            Some(&v) => values.push(v),
                            None => values.push(f32::NAN),
                        }
                        idx += 1;
                    } else {
                        values.push(f32::NAN);
                    }
                }
                values
            }
            None => {
                if decoded.len() == num_points {
                    decoded
                } else {
                    let mut values = decoded;
                    values.resize(num_points, f32::NAN);
                    values
                }
            }
        }
    }
}

// ===========================================================================
// GRIB2 Section 7 data decoders for complex packing (DRT 5.2 / 5.3).
// WMO Manual on Codes Vol. I.2, Templates 5.2 and 5.3.
// ===========================================================================

/// MSB-first bit reader over a byte slice (the GRIB bit convention).
///
/// GRIB2 packs scaled integers most-significant-bit first, with no padding
/// between values. This reader walks the slice bit by bit so values may
/// straddle arbitrary byte boundaries.
pub struct BitReader<'a> {
    /// Backing byte slice being read.
    bytes: &'a [u8],
    /// Absolute bit cursor (0 == MSB of `bytes[0]`).
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    /// Creates a new bit reader positioned at the first bit of `bytes`.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit_pos: 0 }
    }

    /// Reads `n` bits (0..=64) MSB-first.
    ///
    /// Returns `Ok(0)` for `n == 0`. Bounds-checked: a read past the end of
    /// the slice yields [`GribError::TruncatedMessage`]; a request wider than
    /// 64 bits yields [`GribError::InvalidBitOperation`].
    pub fn read_bits(&mut self, n: u32) -> Result<u64> {
        if n == 0 {
            return Ok(0);
        }
        if n > 64 {
            return Err(GribError::InvalidBitOperation(format!(
                "cannot read {n} bits at once (max 64)"
            )));
        }
        let mut value: u64 = 0;
        for _ in 0..n {
            let byte_idx = self.bit_pos / 8;
            if byte_idx >= self.bytes.len() {
                return Err(GribError::TruncatedMessage {
                    expected: byte_idx + 1,
                    actual: self.bytes.len(),
                });
            }
            let bit_in_byte = 7 - (self.bit_pos % 8);
            let bit = (self.bytes[byte_idx] >> bit_in_byte) & 1;
            value = (value << 1) | bit as u64;
            self.bit_pos += 1;
        }
        Ok(value)
    }

    /// Reads `n` bits MSB-first and interprets them as a sign-magnitude
    /// integer: the top bit is the sign (1 == negative), the remaining
    /// `n - 1` bits are the magnitude. This is the WMO convention for the
    /// spatial-differencing extra descriptors of DRT 5.3.
    pub fn read_sign_magnitude(&mut self, n: u32) -> Result<i64> {
        if n == 0 {
            return Ok(0);
        }
        let raw = self.read_bits(n)?;
        let sign_mask = 1u64 << (n - 1);
        let magnitude = (raw & (sign_mask - 1)) as i64;
        if raw & sign_mask != 0 {
            Ok(-magnitude)
        } else {
            Ok(magnitude)
        }
    }

    /// Advances the cursor to the next byte boundary, if not already aligned.
    pub fn align_to_byte(&mut self) {
        if !self.bit_pos.is_multiple_of(8) {
            self.bit_pos = (self.bit_pos / 8 + 1) * 8;
        }
    }

    /// Current absolute bit position.
    pub fn bit_position(&self) -> usize {
        self.bit_pos
    }
}

/// Parameters for DRT 5.2 complex packing (also the 5.3 base).
///
/// Field order mirrors the octet layout of GRIB2 Data Representation
/// Template 5.2 (WMO Manual on Codes Vol. I.2).
#[derive(Debug, Clone)]
pub struct ComplexPackingParams {
    /// Reference value `R` (IEEE-754 f32) — octets 12-15.
    pub reference_value: f32,
    /// Binary scale factor `E` — octets 16-17.
    pub binary_scale_factor: i16,
    /// Decimal scale factor `D` — octets 18-19.
    pub decimal_scale_factor: i16,
    /// Number of bits used for each group reference value — octet 20.
    pub bits_per_value: u8,
    /// Number of groups `NG` into which the data is split — octets 32-35.
    pub num_groups: u32,
    /// Reference value for the group widths — octet 36.
    pub group_widths_reference: u8,
    /// Number of bits used for each scaled group width — octet 37.
    pub group_widths_bits: u8,
    /// Reference value for the group lengths — octets 38-41.
    pub group_lengths_reference: u32,
    /// Length increment for the group lengths — octet 42.
    pub group_length_increment: u8,
    /// True length of the last group — octets 43-46.
    pub group_last_length: u32,
    /// Number of bits used for each scaled group length — octet 47.
    pub group_lengths_bits: u8,
    /// Missing-value management: 0 = none, 1 = primary only, 2 = primary +
    /// secondary — octet 23.
    pub missing_value_management: u8,
    /// Primary missing-value substitute — octets 24-27 (raw bit pattern).
    pub primary_missing_substitute: u32,
    /// Secondary missing-value substitute — octets 28-31 (raw bit pattern).
    pub secondary_missing_substitute: u32,
}

/// Extra parameters for DRT 5.3 spatial differencing.
#[derive(Debug, Clone)]
pub struct SpatialDiffParams {
    /// Order of spatial differencing — octet 48 (1 = first, 2 = second).
    pub order: u8,
    /// Number of octets per extra descriptor — octet 49.
    pub extra_octets: u8,
}

/// Largest raw value representable in `bits` bits, as the all-ones pattern.
/// This is the GRIB sentinel used by the missing-value management feature.
#[inline]
fn all_ones(bits: u32) -> u64 {
    if bits == 0 {
        0
    } else if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

/// Applies the `(R + X * 2^E) / 10^D` GRIB2 scaling formula.
#[inline]
fn apply_scale(x: f64, reference_value: f32, binary_scale: i16, decimal_scale: i16) -> f32 {
    let two_e = 2.0f64.powi(binary_scale as i32);
    let ten_d = 10.0f64.powi(decimal_scale as i32);
    ((reference_value as f64 + x * two_e) / ten_d) as f32
}

/// Group descriptor: reference value, bit width, and member count.
struct GroupDescriptor {
    /// Group reference value `X1` (added to each member residual).
    reference: u64,
    /// Bit width of each member residual within the group.
    width: u32,
    /// Number of values that belong to this group.
    length: usize,
}

/// Reads the per-group descriptor arrays (references, widths, lengths) that
/// precede the packed residuals in DRT 5.2 / 5.3 Section 7 data.
///
/// The three arrays are stored back to back, each byte-aligned at its start
/// per the GRIB convention. Returns the descriptors plus the total point
/// count implied by the group lengths.
fn read_group_descriptors(
    reader: &mut BitReader<'_>,
    params: &ComplexPackingParams,
) -> Result<(Vec<GroupDescriptor>, usize)> {
    let ng = params.num_groups as usize;
    if ng == 0 {
        return Err(GribError::InvalidDataRepresentation(
            "complex packing: number of groups is zero".to_string(),
        ));
    }

    // Array 1: NG group reference values, each `bits_per_value` bits wide.
    let mut references = Vec::with_capacity(ng);
    for _ in 0..ng {
        references.push(reader.read_bits(params.bits_per_value as u32)?);
    }
    reader.align_to_byte();

    // Array 2: NG scaled group widths, each `group_widths_bits` wide. The
    // true width is `reference + scaled`.
    let mut widths = Vec::with_capacity(ng);
    for _ in 0..ng {
        let scaled = reader.read_bits(params.group_widths_bits as u32)?;
        let width = params.group_widths_reference as u64 + scaled;
        if width > 64 {
            return Err(GribError::InvalidDataRepresentation(format!(
                "complex packing: group width {width} exceeds 64 bits"
            )));
        }
        widths.push(width as u32);
    }
    reader.align_to_byte();

    // Array 3: NG scaled group lengths, each `group_lengths_bits` wide. The
    // true length is `reference + increment * scaled`, except the final group
    // whose length is given explicitly by `group_last_length`.
    let mut lengths = Vec::with_capacity(ng);
    for g in 0..ng {
        let scaled = reader.read_bits(params.group_lengths_bits as u32)?;
        let length = if g == ng - 1 {
            params.group_last_length as usize
        } else {
            params.group_lengths_reference as usize
                + params.group_length_increment as usize * scaled as usize
        };
        lengths.push(length);
    }
    reader.align_to_byte();

    let total_points: usize = lengths.iter().sum();
    let mut descriptors = Vec::with_capacity(ng);
    for g in 0..ng {
        descriptors.push(GroupDescriptor {
            reference: references[g],
            width: widths[g],
            length: lengths[g],
        });
    }
    Ok((descriptors, total_points))
}

/// Reads the packed residuals for every group and assembles the raw integer
/// value array `X` (group reference plus per-member residual).
///
/// `missing` carries the resolved missing-value sentinels when missing-value
/// management is active; a group whose reference equals a sentinel (width 0)
/// or a member whose residual equals the all-ones sentinel is mapped to the
/// corresponding substitute, returned here as `None`.
fn read_group_values(
    reader: &mut BitReader<'_>,
    descriptors: &[GroupDescriptor],
    params: &ComplexPackingParams,
) -> Result<Vec<Option<i64>>> {
    let total: usize = descriptors.iter().map(|d| d.length).sum();
    let mut values: Vec<Option<i64>> = Vec::with_capacity(total);

    let mvm = params.missing_value_management;

    for desc in descriptors {
        if desc.width == 0 {
            // Width 0: every member equals the group reference value. Under
            // missing-value management a width-0 group whose reference hits
            // the all-ones sentinel (at the group-reference bit width)
            // represents a run of missing values.
            let resolved = resolve_missing_group_reference(desc.reference, mvm, params);
            for _ in 0..desc.length {
                values.push(resolved);
            }
        } else {
            for _ in 0..desc.length {
                let residual = reader.read_bits(desc.width)?;
                let raw = desc.reference + residual;
                let resolved = resolve_missing_member(raw, residual, desc.width, mvm);
                values.push(resolved);
            }
        }
    }
    Ok(values)
}

/// Resolves a width-0 group's reference value against the missing-value
/// sentinels. A width-0 group is a constant run; when missing-value
/// management is active and the group reference equals the all-ones pattern
/// (primary) or all-ones-minus-one (secondary), the whole run is missing.
/// Returns `None` when the run is missing, otherwise the constant value.
fn resolve_missing_group_reference(
    reference: u64,
    mvm: u8,
    params: &ComplexPackingParams,
) -> Option<i64> {
    if mvm == 0 {
        return Some(reference as i64);
    }
    let sentinel = all_ones(params.bits_per_value as u32);
    if mvm >= 1 && reference == sentinel {
        return None;
    }
    if mvm == 2 && reference == sentinel.wrapping_sub(1) {
        return None;
    }
    Some(reference as i64)
}

/// Resolves a packed group member against the missing-value sentinels.
///
/// Per WMO, when missing-value management is active the all-ones residual
/// pattern within a group marks a missing value (primary if `mvm == 1`,
/// primary or secondary if `mvm == 2`). The substitution actually applies to
/// the assembled raw value matching the integer substitute.
fn resolve_missing_member(raw: u64, residual: u64, width: u32, mvm: u8) -> Option<i64> {
    if mvm == 0 {
        return Some(raw as i64);
    }
    let sentinel = all_ones(width);
    if mvm >= 1 && residual == sentinel {
        // Primary (mvm==1) or secondary (mvm==2) missing marker. With
        // mvm==2 the secondary marker is all-ones minus one.
        return None;
    }
    if mvm == 2 && residual == sentinel.wrapping_sub(1) {
        return None;
    }
    Some(raw as i64)
}

/// Decodes DRT 5.2 complex-packed Section 7 data into `f32` values.
///
/// Steps (WMO Manual on Codes Vol. I.2, Template 5.2):
/// 1. read the `NG` group reference / width / length descriptor arrays;
/// 2. read each group's residuals at its own bit width and add the group
///    reference (a width-0 group yields a constant run);
/// 3. apply the `(R + X * 2^E) / 10^D` scaling;
/// 4. honour missing-value management by substituting the configured values.
///
/// `num_points` is the grid point count; the decoder trusts the group
/// lengths but will not emit more than `num_points` values.
pub fn decode_complex_packing(
    data: &[u8],
    params: &ComplexPackingParams,
    num_points: usize,
) -> Result<Vec<f32>> {
    let mut reader = BitReader::new(data);
    let (descriptors, total_points) = read_group_descriptors(&mut reader, params)?;
    let raw_values = read_group_values(&mut reader, &descriptors, params)?;

    let count = total_points.min(num_points);
    let mut out = Vec::with_capacity(count);
    for slot in raw_values.iter().take(count) {
        match slot {
            Some(x) => out.push(apply_scale(
                *x as f64,
                params.reference_value,
                params.binary_scale_factor,
                params.decimal_scale_factor,
            )),
            None => out.push(missing_substitute_value(params)),
        }
    }
    Ok(out)
}

/// Decodes DRT 5.3 complex packing with spatial differencing into `f32`s.
///
/// Steps (WMO Manual on Codes Vol. I.2, Template 5.3):
/// 1. read the spatial-diff extra descriptors: `order` initial values
///    followed by the overall minimum of the differences, each
///    `extra_octets` octets wide and stored sign-magnitude;
/// 2. group-decode the body exactly as DRT 5.2 into the difference array;
/// 3. add the overall minimum back to every difference;
/// 4. invert the differencing:
///    - order 1: `v[i] = d[i] + v[i-1]`;
///    - order 2: `v[i] = d[i] + 2*v[i-1] - v[i-2]`;
/// 5. apply the `(R + v * 2^E) / 10^D` scaling.
pub fn decode_complex_with_spatial_diff(
    data: &[u8],
    params: &ComplexPackingParams,
    sd: &SpatialDiffParams,
    num_points: usize,
) -> Result<Vec<f32>> {
    if sd.order != 1 && sd.order != 2 {
        return Err(GribError::InvalidDataRepresentation(format!(
            "spatial differencing: unsupported order {} (only 1 or 2)",
            sd.order
        )));
    }
    if sd.extra_octets == 0 {
        return Err(GribError::InvalidDataRepresentation(
            "spatial differencing: extra octet count is zero".to_string(),
        ));
    }

    let mut reader = BitReader::new(data);

    // Step 1: the extra descriptors precede the group descriptor arrays.
    // There are `order` initial values followed by one overall minimum.
    let descriptor_bits = sd.extra_octets as u32 * 8;
    let mut initials: Vec<i64> = Vec::with_capacity(sd.order as usize);
    for _ in 0..sd.order {
        initials.push(reader.read_sign_magnitude(descriptor_bits)?);
    }
    let overall_minimum = reader.read_sign_magnitude(descriptor_bits)?;
    reader.align_to_byte();

    // Step 2: group-decode the differences. Missing-value management is not
    // combined with spatial differencing in practice, but the same group
    // machinery is reused; a missing slot is treated as a zero difference.
    let (descriptors, total_points) = read_group_descriptors(&mut reader, params)?;
    let raw_values = read_group_values(&mut reader, &descriptors, params)?;

    // Step 3: add the overall minimum back to every decoded difference.
    let mut diffs: Vec<i64> = Vec::with_capacity(raw_values.len());
    for slot in &raw_values {
        match slot {
            Some(x) => diffs.push(*x + overall_minimum),
            None => diffs.push(overall_minimum),
        }
    }

    // Step 4: invert the spatial differencing in place. The first `order`
    // values are the stored initial values, not differences.
    let count = total_points.min(num_points);
    let mut values: Vec<i64> = Vec::with_capacity(count);
    match sd.order {
        1 => {
            for (i, slot) in diffs.iter().enumerate().take(count) {
                if i == 0 {
                    values.push(initials[0]);
                } else {
                    let prev = values[i - 1];
                    values.push(slot + prev);
                }
            }
        }
        _ => {
            // order == 2
            for (i, slot) in diffs.iter().enumerate().take(count) {
                match i {
                    0 => values.push(initials[0]),
                    1 => values.push(initials[1]),
                    _ => {
                        let v1 = values[i - 1];
                        let v2 = values[i - 2];
                        values.push(slot + 2 * v1 - v2);
                    }
                }
            }
        }
    }

    // Step 5: apply the scaling formula.
    let mut out = Vec::with_capacity(values.len());
    for v in values {
        out.push(apply_scale(
            v as f64,
            params.reference_value,
            params.binary_scale_factor,
            params.decimal_scale_factor,
        ));
    }
    Ok(out)
}

/// Produces the scaled `f32` for a missing data point. The primary missing
/// substitute (interpreted as an IEEE-754 bit pattern when present) is used;
/// failing that, NaN is returned, which is the conventional GRIB sentinel.
fn missing_substitute_value(params: &ComplexPackingParams) -> f32 {
    if params.missing_value_management == 0 {
        return f32::NAN;
    }
    // The substitute is documented as a value in the same representation as
    // the field; the safest cross-implementation choice is NaN so callers can
    // detect gaps. A non-NaN substitute encoded as an f32 bit pattern is
    // surfaced when it is finite.
    let candidate = f32::from_bits(params.primary_missing_substitute);
    if candidate.is_finite() {
        candidate
    } else {
        f32::NAN
    }
}
