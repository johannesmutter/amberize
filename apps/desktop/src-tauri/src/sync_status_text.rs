/// Sync-status formatting helpers shared between background sync and commands.
///
/// We keep `last_sync_at` machine-readable (RFC3339) for the UI and store a
/// human-readable `last_sync_status` for display (tray menu, etc.).
pub fn format_last_sync_status_text(status: &str, timestamp_rfc3339: &str) -> String {
    let status_label = normalize_status_label(status);
    let Some(when) = format_timestamp_compact(timestamp_rfc3339) else {
        return status_label;
    };

    format!("{status_label} — {when}")
}

fn normalize_status_label(status: &str) -> String {
    let normalized = status.trim();
    if normalized.is_empty() {
        return "Unknown".to_string();
    }

    match normalized.to_ascii_lowercase().as_str() {
        "ok" => "OK".to_string(),
        "partial" => "Partial".to_string(),
        "error" => "Error".to_string(),
        "never" => "Never".to_string(),
        "not configured" => "Not configured".to_string(),
        other => capitalize_first_ascii(other),
    }
}

fn capitalize_first_ascii(input: &str) -> String {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    let mut out = String::new();
    out.extend(first.to_uppercase());
    out.push_str(chars.as_str());
    out
}

fn format_timestamp_compact(timestamp_rfc3339: &str) -> Option<String> {
    let ts = timestamp_rfc3339.trim();
    if ts.is_empty() {
        return None;
    }

    let (base, offset) = split_rfc3339_base_and_offset(ts);

    let without_fraction = base.split('.').next().unwrap_or(base);
    let mut base = without_fraction.replace('T', " ");

    // Prefer a compact `YYYY-MM-DD HH:MM` display in tray menus.
    if base.len() >= 16 {
        base.truncate(16);
    }

    let offset_label = match offset {
        "Z" => "UTC",
        "" => "",
        other => other,
    };

    if offset_label.is_empty() {
        return Some(base);
    }

    Some(format!("{base} {offset_label}"))
}

fn split_rfc3339_base_and_offset(ts: &str) -> (&str, &str) {
    if let Some(z_pos) = ts.find('Z') {
        return (&ts[..z_pos], "Z");
    }

    if let Some(plus_pos) = ts.rfind('+') {
        // Avoid matching a '+' in unexpected places; RFC3339 offsets are suffixes.
        if plus_pos > 10 {
            return (&ts[..plus_pos], &ts[plus_pos..]);
        }
    }

    // Offset may be negative: search only after the `T` separator to avoid the date portion.
    if let Some(t_pos) = ts.find('T') {
        let after_t = &ts[(t_pos + 1)..];
        if let Some(minus_rel) = after_t.rfind('-') {
            let minus_pos = t_pos + 1 + minus_rel;
            // Ensure we have at least `HH:MM` before treating this as an offset sign.
            if minus_pos >= t_pos + 1 + 5 {
                return (&ts[..minus_pos], &ts[minus_pos..]);
            }
        }
    }

    (ts, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_ok_without_fraction() {
        let out = format_last_sync_status_text("ok", "2026-02-05T20:29:14Z");
        assert_eq!(out, "OK — 2026-02-05 20:29 UTC");
    }

    #[test]
    fn formats_partial_with_fraction() {
        let out = format_last_sync_status_text("partial", "2026-02-05T20:29:14.188532Z");
        assert_eq!(out, "Partial — 2026-02-05 20:29 UTC");
    }

    #[test]
    fn formats_unknown_status() {
        let out = format_last_sync_status_text("weird", "2026-02-05T20:29:14Z");
        assert_eq!(out, "Weird — 2026-02-05 20:29 UTC");
    }

    #[test]
    fn returns_label_when_timestamp_missing() {
        let out = format_last_sync_status_text("partial", "   ");
        assert_eq!(out, "Partial");
    }
}
