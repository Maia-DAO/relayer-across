use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub fn rfc3339_to_unix(input: &str) -> Result<u64, String> {
    let dt = OffsetDateTime::parse(input, &Rfc3339)
        .map_err(|e| format!("Parse timestamp error: {}", e))?;

    let ts = dt.unix_timestamp();

    if ts >= 0 {
        Ok(ts as u64)
    } else {
        Err("Timestamp is negative (before 1970-01-01)".into())
    }
}
