//! Byte and rate formatting in base 1024, and small layout helpers.

pub const KB: f64 = 1024.0;
pub const MB: f64 = 1024.0 * 1024.0;
pub const GB: f64 = 1024.0 * 1024.0 * 1024.0;
pub const TB: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0;

/// Humanize a byte-per-second rate using base-1024 units.
pub fn humanize_rate(bps: u64) -> String {
    let b = bps as f64;
    if b >= GB {
        format!("{:.2} GB/s", b / GB)
    } else if b >= MB {
        format!("{:.2} MB/s", b / MB)
    } else if b >= KB {
        format!("{:.1} KB/s", b / KB)
    } else {
        format!("{bps} B/s")
    }
}

/// Humanize a plain byte total using base-1024 units.
pub fn humanize_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= TB {
        format!("{:.2} TB", b / TB)
    } else if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Compact rate for a plot axis mark: the unit is picked per mark and
/// trailing zeros are dropped, so the axis reads "1.5 MB/s" not "1572864".
pub fn axis_rate(bps: f64) -> String {
    // Marks below zero come from the plot's margin and get no label, so an
    // idle process does not show a column of zeros.
    if bps < 0.0 {
        return String::new();
    }
    if bps == 0.0 {
        return "0".to_string();
    }
    let (value, unit) = if bps >= GB {
        (bps / GB, "GB/s")
    } else if bps >= MB {
        (bps / MB, "MB/s")
    } else if bps >= KB {
        (bps / KB, "KB/s")
    } else {
        (bps, "B/s")
    };
    let text = format!("{value:.1}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text} {unit}")
}

/// A remembered size (a column width or a panel size, in points) settled to
/// the nearest half point. Returns the value to store when `seen` is a real
/// size and differs from `saved` by at least half a point, or when nothing
/// was saved yet; otherwise `None`, so a drag that barely moves writes
/// nothing.
pub fn settle(saved: Option<f32>, seen: f32) -> Option<f32> {
    if !seen.is_finite() || seen <= 0.0 {
        return None;
    }
    let seen = (seen * 2.0).round() / 2.0;
    saved
        .is_none_or(|v| (v - seen).abs() >= 0.5)
        .then_some(seen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_axis_marks_get_no_label() {
        assert_eq!(axis_rate(-512.0), "");
        assert_eq!(axis_rate(0.0), "0");
    }

    #[test]
    fn axis_rates_pick_a_unit_and_trim_zeros() {
        assert_eq!(axis_rate(0.0), "0");
        assert_eq!(axis_rate(512.0), "512 B/s");
        assert_eq!(axis_rate(1536.0), "1.5 KB/s");
        assert_eq!(axis_rate(2.0 * MB), "2 MB/s");
        assert_eq!(axis_rate(1.75 * GB), "1.8 GB/s");
    }

    #[test]
    fn rates_and_bytes_pick_a_unit() {
        assert_eq!(humanize_rate(512), "512 B/s");
        assert_eq!(humanize_rate(1536), "1.5 KB/s");
        assert_eq!(humanize_rate(2 * 1024 * 1024), "2.00 MB/s");
        assert_eq!(humanize_bytes(1023), "1023 B");
        assert_eq!(humanize_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
        assert_eq!(humanize_bytes(1024u64.pow(4)), "1.00 TB");
    }

    #[test]
    fn settle_records_the_first_value() {
        assert_eq!(settle(None, 120.0), Some(120.0));
    }

    #[test]
    fn settle_records_a_changed_value() {
        assert_eq!(settle(Some(120.0), 140.0), Some(140.0));
        assert_eq!(settle(Some(120.0), 120.5), Some(120.5));
    }

    #[test]
    fn settle_ignores_a_change_within_the_tolerance() {
        assert_eq!(settle(Some(120.0), 120.0), None);
        assert_eq!(settle(Some(120.0), 120.2), None);
        assert_eq!(settle(Some(120.0), 119.8), None);
    }

    #[test]
    fn settle_rejects_zero_negative_and_nan() {
        assert_eq!(settle(None, 0.0), None);
        assert_eq!(settle(None, -4.0), None);
        assert_eq!(settle(None, f32::NAN), None);
        assert_eq!(settle(None, f32::INFINITY), None);
    }

    #[test]
    fn settle_rounds_to_halves() {
        assert_eq!(settle(None, 100.3), Some(100.5));
        assert_eq!(settle(None, 100.2), Some(100.0));
        assert_eq!(settle(None, 100.8), Some(101.0));
    }
}
