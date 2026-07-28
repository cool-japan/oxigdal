//! GRIB2 Section 4: Product Definition Section

use crate::error::{GribError, Result};
use crate::templates::StatisticalMethod;
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

/// Ensemble-forecast metadata carried by PDT 4.1 / 4.11
/// (WMO Manual on Codes Vol. I.2, Template 4.1, octets 35-37).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsembleInfo {
    /// Type of ensemble forecast (WMO Code Table 4.6) — octet 35.
    pub ensemble_type: u8,
    /// Perturbation number identifying the member — octet 36.
    pub perturbation_number: u8,
    /// Total number of forecasts in the ensemble — octet 37.
    pub num_forecasts: u8,
}

/// A single time-range specification within a statistical-processing PDT
/// (WMO Manual on Codes Vol. I.2, Template 4.8, the repeated 12-octet block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeRangeSpec {
    /// Statistical process code (WMO Code Table 4.10) — first octet of the block.
    pub statistical_process: u8,
    /// Type of time increment between successive fields (WMO Code Table 4.11).
    pub time_increment_type: u8,
    /// Indicator of the unit of time for the length of the time range
    /// (WMO Code Table 4.4).
    pub time_range_unit: u8,
    /// Length of the time range over which processing is done, in the unit
    /// given by `time_range_unit`.
    pub time_range_length: u32,
    /// Indicator of the unit of time for the increment between fields.
    pub increment_unit: u8,
    /// Time increment between successive fields, in `increment_unit`.
    pub time_increment: u32,
}

impl TimeRangeSpec {
    /// Returns the statistical-processing method as a typed
    /// [`StatisticalMethod`] (WMO Code Table 4.10).
    #[must_use]
    pub fn method(&self) -> StatisticalMethod {
        StatisticalMethod::from_u8(self.statistical_process)
    }
}

/// Statistical-processing metadata carried by PDT 4.8 / 4.9 / 4.10 / 4.11 / 4.12
/// (average / accumulation / extreme over a time interval). Modelled here for
/// PDT 4.8 (WMO Manual on Codes Vol. I.2, Template 4.8, octets 35-58+).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatisticalProcessInfo {
    /// Year of the end of the overall time interval — octets 35-36.
    pub end_year: u16,
    /// Month of the end of the overall time interval — octet 37.
    pub end_month: u8,
    /// Day of the end of the overall time interval — octet 38.
    pub end_day: u8,
    /// Hour of the end of the overall time interval — octet 39.
    pub end_hour: u8,
    /// Minute of the end of the overall time interval — octet 40.
    pub end_minute: u8,
    /// Second of the end of the overall time interval — octet 41.
    pub end_second: u8,
    /// Total number of data values missing in the statistical process —
    /// octets 43-46.
    pub num_missing: u32,
    /// Per-range time-range specifications — the `n` repeated 12-octet blocks.
    pub time_ranges: Vec<TimeRangeSpec>,
}

impl StatisticalProcessInfo {
    /// Returns the end of the overall statistical time interval as a
    /// [`chrono::NaiveDateTime`], or `None` if the stored fields are not a
    /// valid calendar date/time.
    #[must_use]
    pub fn interval_end_time(&self) -> Option<chrono::NaiveDateTime> {
        let date = chrono::NaiveDate::from_ymd_opt(
            self.end_year as i32,
            self.end_month as u32,
            self.end_day as u32,
        )?;
        date.and_hms_opt(
            self.end_hour as u32,
            self.end_minute as u32,
            self.end_second as u32,
        )
    }

    /// Returns the statistical method of the first (usually only) time-range
    /// specification, if present.
    #[must_use]
    pub fn primary_method(&self) -> Option<StatisticalMethod> {
        self.time_ranges.first().map(TimeRangeSpec::method)
    }
}

/// GRIB2 Section 4: Product Definition Section
///
/// Contains information about the meteorological parameter, forecast time,
/// and vertical level. The common PDT 4.0 prefix (parameter, generating
/// process, forecast time, and both fixed surfaces) is populated for every
/// supported template; template-specific trailing metadata is surfaced through
/// [`ProductDefinitionSection::ensemble`] (PDT 4.1) and
/// [`ProductDefinitionSection::statistical_process`] (PDT 4.8).
#[derive(Debug, Clone)]
pub struct ProductDefinitionSection {
    /// Product definition template number (PDT 4.x).
    pub template_number: u16,
    /// Parameter category (e.g., temperature, moisture)
    pub parameter_category: u8,
    /// Parameter number within category
    pub parameter_number: u8,
    /// Type of generating process
    pub generating_process: u8,
    /// Indicator of unit of time range (WMO Code Table 4.4) applied to
    /// `forecast_time`.
    pub time_range_unit: u8,
    /// Forecast time in units indicated by `time_range_unit`
    pub forecast_time: u32,
    /// Type of first fixed surface
    pub first_surface_type: u8,
    /// Value of first fixed surface
    pub first_surface_value: f64,
    /// Type of second fixed surface (WMO Code Table 4.5), `None` when missing.
    pub second_surface_type: Option<u8>,
    /// Value of second fixed surface, `None` when missing.
    pub second_surface_value: Option<f64>,
    /// Ensemble metadata (present for PDT 4.1).
    pub ensemble: Option<EnsembleInfo>,
    /// Statistical-processing metadata (present for PDT 4.8).
    pub statistical_process: Option<StatisticalProcessInfo>,
}

/// Reads a fixed-surface `(type, scale factor, scaled value)` triple and
/// resolves it to `(type, value)` per WMO PDT 4.0 (octets 23-28 for the first
/// surface, 29-34 for the second).
///
/// The scale factor is a WMO `signed[1]` (sign-and-magnitude) field; a missing
/// scale factor (`0xFF`) means "no scaling", and a missing scaled value
/// (`u32::MAX`) has no defined physical magnitude and decodes to `0.0`,
/// matching eccodes' `G2Level::unpack_double`.
fn read_fixed_surface(cursor: &mut Cursor<&[u8]>) -> Result<(u8, f64)> {
    let surface_type = cursor.read_u8()?;
    let scale_factor_raw = cursor.read_u8()?;
    let scale_factor_missing = scale_factor_raw == 0xFF;
    let scale_factor = if scale_factor_raw & 0x80 != 0 {
        -((scale_factor_raw & 0x7F) as i32)
    } else {
        scale_factor_raw as i32
    };
    let scaled_value = cursor.read_u32::<BigEndian>()?;
    let value = if scaled_value == u32::MAX {
        0.0
    } else if scale_factor_missing {
        scaled_value as f64
    } else {
        scaled_value as f64 / 10f64.powi(scale_factor)
    };
    Ok((surface_type, value))
}

impl ProductDefinitionSection {
    /// Parses the product definition section from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = Cursor::new(data);

        let _num_coordinates = cursor.read_u16::<BigEndian>()?;
        let template_number = cursor.read_u16::<BigEndian>()?;

        match template_number {
            0 | 1 | 8 => Self::parse_common_templates(&mut cursor, template_number),
            _ => Err(GribError::UnsupportedProductTemplate(template_number)),
        }
    }

    /// Parses PDT 4.0 / 4.1 / 4.8, all of which share the same common prefix
    /// (WMO Manual on Codes Vol. I.2). The cursor must be positioned
    /// immediately after the template number (octet 10 of the section).
    fn parse_common_templates(cursor: &mut Cursor<&[u8]>, template_number: u16) -> Result<Self> {
        // --- Common PDT 4.0 prefix (octets 10-34) ---
        let parameter_category = cursor.read_u8()?;
        let parameter_number = cursor.read_u8()?;
        let generating_process = cursor.read_u8()?;
        let _background = cursor.read_u8()?;
        let _analysis = cursor.read_u8()?;
        let _hours_cutoff = cursor.read_u16::<BigEndian>()?;
        let _minutes_cutoff = cursor.read_u8()?;
        let time_range_unit = cursor.read_u8()?;
        let forecast_time = cursor.read_u32::<BigEndian>()?;
        let (first_surface_type, first_surface_value) = read_fixed_surface(cursor)?;
        let (second_type_raw, second_value) = read_fixed_surface(cursor)?;
        // A second fixed surface with a missing type (0xFF) means "no second
        // surface" (WMO Regulation 92.1.12).
        let (second_surface_type, second_surface_value) = if second_type_raw == 0xFF {
            (None, None)
        } else {
            (Some(second_type_raw), Some(second_value))
        };

        // --- Template-specific trailing fields ---
        let mut ensemble = None;
        let mut statistical_process = None;
        match template_number {
            1 => ensemble = Some(Self::parse_ensemble(cursor)?),
            8 => statistical_process = Some(Self::parse_statistical_process(cursor)?),
            _ => {}
        }

        Ok(Self {
            template_number,
            parameter_category,
            parameter_number,
            generating_process,
            time_range_unit,
            forecast_time,
            first_surface_type,
            first_surface_value,
            second_surface_type,
            second_surface_value,
            ensemble,
            statistical_process,
        })
    }

    /// Parses the PDT 4.1 ensemble trailer (octets 35-37).
    fn parse_ensemble(cursor: &mut Cursor<&[u8]>) -> Result<EnsembleInfo> {
        let ensemble_type = cursor.read_u8()?;
        let perturbation_number = cursor.read_u8()?;
        let num_forecasts = cursor.read_u8()?;
        Ok(EnsembleInfo {
            ensemble_type,
            perturbation_number,
            num_forecasts,
        })
    }

    /// Parses the PDT 4.8 statistical-processing trailer (octets 35-58+): the
    /// end-of-interval date/time, the number of missing values, and the `n`
    /// repeated 12-octet time-range specifications.
    fn parse_statistical_process(cursor: &mut Cursor<&[u8]>) -> Result<StatisticalProcessInfo> {
        // Octets 35-41: end of overall time interval.
        let end_year = cursor.read_u16::<BigEndian>()?;
        let end_month = cursor.read_u8()?;
        let end_day = cursor.read_u8()?;
        let end_hour = cursor.read_u8()?;
        let end_minute = cursor.read_u8()?;
        let end_second = cursor.read_u8()?;
        // Octet 42: n, number of time-range specifications.
        let num_ranges = cursor.read_u8()? as usize;
        // Octets 43-46: total number of data values missing.
        let num_missing = cursor.read_u32::<BigEndian>()?;

        let mut time_ranges = Vec::with_capacity(num_ranges);
        for _ in 0..num_ranges {
            let statistical_process = cursor.read_u8()?;
            let time_increment_type = cursor.read_u8()?;
            let time_range_unit = cursor.read_u8()?;
            let time_range_length = cursor.read_u32::<BigEndian>()?;
            let increment_unit = cursor.read_u8()?;
            let time_increment = cursor.read_u32::<BigEndian>()?;
            time_ranges.push(TimeRangeSpec {
                statistical_process,
                time_increment_type,
                time_range_unit,
                time_range_length,
                increment_unit,
                time_increment,
            });
        }

        Ok(StatisticalProcessInfo {
            end_year,
            end_month,
            end_day,
            end_hour,
            end_minute,
            end_second,
            num_missing,
            time_ranges,
        })
    }

    /// Returns `true` if this product is a statistical quantity accumulated,
    /// averaged, or otherwise processed over a time interval (PDT 4.8), rather
    /// than an instantaneous value (PDT 4.0).
    #[must_use]
    pub fn is_time_interval(&self) -> bool {
        self.statistical_process.is_some()
    }

    /// Returns the statistical method (WMO Code Table 4.10) for a
    /// time-interval product, if any.
    #[must_use]
    pub fn statistical_method(&self) -> Option<StatisticalMethod> {
        self.statistical_process
            .as_ref()
            .and_then(StatisticalProcessInfo::primary_method)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Builds a minimal PDT 4.0 byte buffer with the given raw scale-factor
    /// and scaled-value octets for the first fixed surface. The second fixed
    /// surface is written as "missing" (type 0xFF).
    fn pdt0_bytes(surface_type: u8, scale_factor_raw: u8, scaled_value: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_be_bytes()); // num_coordinates
        data.extend_from_slice(&0u16.to_be_bytes()); // template 4.0
        data.push(0); // parameter_category
        data.push(0); // parameter_number
        data.push(0); // generating_process
        data.push(0); // background
        data.push(0); // analysis
        data.extend_from_slice(&0u16.to_be_bytes()); // hours cutoff
        data.push(0); // minutes cutoff
        data.push(1); // time unit
        data.extend_from_slice(&0u32.to_be_bytes()); // forecast_time
        data.push(surface_type); // first_surface_type
        data.push(scale_factor_raw); // scale factor of first fixed surface
        data.extend_from_slice(&scaled_value.to_be_bytes()); // scaled value
        // Second fixed surface: missing.
        data.push(0xFF); // second surface type: missing
        data.push(0xFF); // scale factor: missing
        data.extend_from_slice(&u32::MAX.to_be_bytes()); // scaled value: missing
        data
    }

    #[test]
    fn test_first_surface_value_applies_positive_scale_factor() {
        let data = pdt0_bytes(100, 0x02, 850);
        let pds = ProductDefinitionSection::from_bytes(&data)
            .expect("failed to parse PDT 4.0 with positive scale factor");
        assert!((pds.first_surface_value - 8.5).abs() < 1e-9);
        assert_eq!(pds.template_number, 0);
        assert!(pds.second_surface_type.is_none());
        assert!(!pds.is_time_interval());
    }

    #[test]
    fn test_first_surface_value_applies_negative_scale_factor() {
        let data = pdt0_bytes(100, 0x82, 5);
        let pds = ProductDefinitionSection::from_bytes(&data)
            .expect("failed to parse PDT 4.0 with negative scale factor");
        assert!((pds.first_surface_value - 500.0).abs() < 1e-9);
    }

    #[test]
    fn test_first_surface_value_zero_scale_factor_is_identity() {
        let data = pdt0_bytes(1, 0x00, 42);
        let pds = ProductDefinitionSection::from_bytes(&data)
            .expect("failed to parse PDT 4.0 with zero scale factor");
        assert!((pds.first_surface_value - 42.0).abs() < 1e-9);
    }

    #[test]
    fn test_first_surface_value_missing_scaled_value_decodes_to_zero() {
        let data = pdt0_bytes(100, 0x00, u32::MAX);
        let pds = ProductDefinitionSection::from_bytes(&data)
            .expect("failed to parse PDT 4.0 with missing scaled value");
        assert_eq!(pds.first_surface_value, 0.0);
    }

    #[test]
    fn test_first_surface_value_missing_scale_factor_is_unscaled() {
        let data = pdt0_bytes(100, 0xFF, 12345);
        let pds = ProductDefinitionSection::from_bytes(&data)
            .expect("failed to parse PDT 4.0 with missing scale factor");
        assert!((pds.first_surface_value - 12345.0).abs() < 1e-9);
    }

    #[test]
    fn test_second_fixed_surface_present() {
        let mut data = pdt0_bytes(100, 0x00, 100000);
        // Overwrite the missing second surface with a real one: isobaric,
        // scale 0, value 50000 (Pa).
        let n = data.len();
        data[n - 6] = 100; // second surface type
        data[n - 5] = 0; // scale factor
        data[(n - 4)..].copy_from_slice(&50000u32.to_be_bytes());
        let pds = ProductDefinitionSection::from_bytes(&data).expect("parse failed");
        assert_eq!(pds.second_surface_type, Some(100));
        assert!((pds.second_surface_value.unwrap() - 50000.0).abs() < 1e-6);
    }

    /// PDT 4.1 (individual ensemble forecast): the common prefix plus the
    /// ensemble-type / perturbation-number / ensemble-size trailer.
    #[test]
    fn test_pdt1_ensemble_fields_parsed() {
        let mut data = pdt0_bytes(103, 0x00, 2);
        // Switch template number to 1.
        data[2..4].copy_from_slice(&1u16.to_be_bytes());
        // Append the ensemble trailer.
        data.push(3); // ensemble type
        data.push(7); // perturbation number
        data.push(20); // number of forecasts
        let pds = ProductDefinitionSection::from_bytes(&data).expect("PDT 4.1 parse failed");
        assert_eq!(pds.template_number, 1);
        let ens = pds.ensemble.as_ref().expect("ensemble info missing");
        assert_eq!(ens.ensemble_type, 3);
        assert_eq!(ens.perturbation_number, 7);
        assert_eq!(ens.num_forecasts, 20);
        assert!(pds.statistical_process.is_none());
    }

    /// PDT 4.8 (statistical processing over a time interval): the common
    /// prefix plus the end-of-interval date, the missing count, and one
    /// 12-octet time-range specification (accumulation).
    #[test]
    fn test_pdt8_statistical_process_parsed() {
        let mut data = pdt0_bytes(1, 0x00, 0);
        data[2..4].copy_from_slice(&8u16.to_be_bytes());
        // End of overall time interval: 2024-01-02 06:00:00.
        data.extend_from_slice(&2024u16.to_be_bytes());
        data.push(1); // month
        data.push(2); // day
        data.push(6); // hour
        data.push(0); // minute
        data.push(0); // second
        data.push(1); // n = 1 time-range spec
        data.extend_from_slice(&0u32.to_be_bytes()); // num missing
        // One time-range spec: accumulation (1), increment type 2, unit hour
        // (1), range length 6, increment unit hour (1), increment 0.
        data.push(1); // statistical process = accumulation
        data.push(2); // time increment type
        data.push(1); // time range unit (hour)
        data.extend_from_slice(&6u32.to_be_bytes()); // range length
        data.push(1); // increment unit
        data.extend_from_slice(&0u32.to_be_bytes()); // increment

        let pds = ProductDefinitionSection::from_bytes(&data).expect("PDT 4.8 parse failed");
        assert_eq!(pds.template_number, 8);
        assert!(pds.is_time_interval());
        let stat = pds.statistical_process.as_ref().expect("stat info missing");
        assert_eq!(stat.end_year, 2024);
        assert_eq!(stat.end_month, 1);
        assert_eq!(stat.end_day, 2);
        assert_eq!(stat.end_hour, 6);
        assert_eq!(stat.time_ranges.len(), 1);
        assert_eq!(
            stat.time_ranges[0].method(),
            StatisticalMethod::Accumulation
        );
        assert_eq!(
            pds.statistical_method(),
            Some(StatisticalMethod::Accumulation)
        );
        let end = stat.interval_end_time().expect("valid end time");
        assert_eq!(
            end,
            chrono::NaiveDate::from_ymd_opt(2024, 1, 2)
                .unwrap()
                .and_hms_opt(6, 0, 0)
                .unwrap()
        );
    }
}
