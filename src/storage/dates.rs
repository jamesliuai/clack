use super::{ErrorKind, StorageError};

/// UTC date bounds. Dates are inclusive from / exclusive following day to.
/// Full timestamps accept the deliberately small RFC3339 UTC subset ending Z.
pub fn parse_utc_bound(value: &str, end: bool) -> Result<i64, StorageError> {
    let invalid = || {
        StorageError::new(
            ErrorKind::Invalid,
            "date expects YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS[.sss]Z in UTC",
        )
    };
    if !value.is_ascii() || value.len() < 10 {
        return Err(invalid());
    }
    let number = |range: std::ops::Range<usize>| -> Result<i64, StorageError> {
        let text = value.get(range).ok_or_else(invalid)?;
        if !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid());
        }
        text.parse().map_err(|_| invalid())
    };
    if &value[4..5] != "-" || &value[7..8] != "-" {
        return Err(invalid());
    }
    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return Err(invalid()),
    };
    if year < 1 || !(1..=days).contains(&day) {
        return Err(invalid());
    }
    let mut stamp = days_from_civil(year, month, day) * 86_400_000;
    if value.len() == 10 {
        return Ok(stamp + if end { 86_400_000 } else { 0 });
    }
    if !matches!(value.len(), 20 | 22 | 23 | 24)
        || &value[10..11] != "T"
        || &value[13..14] != ":"
        || &value[16..17] != ":"
        || !value.ends_with('Z')
    {
        return Err(invalid());
    }
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    if hour > 23 || minute > 59 || second > 59 {
        return Err(invalid());
    }
    stamp += ((hour * 60 + minute) * 60 + second) * 1000;
    if value.len() > 20 {
        if &value[19..20] != "." {
            return Err(invalid());
        }
        let digits = value.len() - 21;
        stamp += number(20..value.len() - 1)? * 10_i64.pow((3 - digits) as u32);
    }
    Ok(stamp)
}

/// Gregorian calendar conversion using 400-year eras and Euclidean division.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + day_of_year - 719_468
}

pub fn format_utc_millis(stamp: i64) -> String {
    let days = stamp.div_euclid(86_400_000);
    let time = stamp.rem_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        time / 3_600_000,
        (time / 60_000) % 60,
        (time / 1000) % 60,
        time % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utc_dates_are_strict_and_round_trip_calendar_edges() {
        assert_eq!(parse_utc_bound("1970-01-01", false).unwrap(), 0);
        assert_eq!(format_utc_millis(-1), "1969-12-31T23:59:59.999Z");
        for date in [
            "0001-01-01T00:00:00.000Z",
            "1900-02-28T23:59:59.999Z",
            "2000-02-29T12:34:56.123Z",
            "2024-12-31T23:59:59.999Z",
            "9999-12-31T23:59:59.999Z",
        ] {
            assert_eq!(
                format_utc_millis(parse_utc_bound(date, false).unwrap()),
                date
            );
        }
        for invalid in [
            "2023-02-29",
            "1900-02-29",
            "2024-00-01",
            "2024-01-00",
            "2024-01-32",
            "2024-01-01T24:00:00Z",
            "2024-01-01T00:00:60Z",
            "2024-01-01T00:00:00+01:00",
            "秘密2024",
            "2024-01-01T00:00:00.1234Z",
        ] {
            assert!(parse_utc_bound(invalid, false).is_err());
        }
        assert_eq!(
            parse_utc_bound("2024-02-29", true).unwrap(),
            parse_utc_bound("2024-03-01", false).unwrap()
        );
    }
}
