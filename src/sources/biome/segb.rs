//! SEGB v1/v2 framing. Format references: CCL Forensics' ccl_segb and
//! https://cellebrite.com/en/understanding-and-decoding-the-newest-ios-segb-format/
//! See FORMAT_LICENSE for the reference implementation's attribution.
use crate::sources::local_store::apple_time;
use anyhow::{ensure, Context};
use chrono::{DateTime, Utc};

pub(super) struct Record<'a> {
    pub offset: usize,
    pub timestamp: DateTime<Utc>,
    pub end_timestamp: Option<DateTime<Utc>>,
    pub crc_valid: bool,
    pub data: &'a [u8],
}
fn array<const N: usize>(b: &[u8], offset: usize) -> anyhow::Result<[u8; N]> {
    b.get(offset..offset.checked_add(N).context("SEGB offset overflow")?)
        .context("Truncated SEGB record")?
        .try_into()
        .context("Truncated SEGB field")
}
fn u32_at(b: &[u8], o: usize) -> anyhow::Result<u32> {
    Ok(u32::from_le_bytes(array(b, o)?))
}
fn f64_at(b: &[u8], o: usize) -> anyhow::Result<f64> {
    Ok(f64::from_le_bytes(array(b, o)?))
}
fn time(b: &[u8], o: usize) -> anyhow::Result<DateTime<Utc>> {
    apple_time(Some(f64_at(b, o)?))?.context("Missing SEGB timestamp")
}

pub(super) fn parse<'a>(
    b: &'a [u8],
    mut emit: impl FnMut(Record<'a>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    if b.get(52..56) == Some(b"SEGB") {
        let end = u32_at(b, 0)? as usize;
        ensure!(end >= 56 && end <= b.len(), "Invalid SEGB v1 data boundary");
        let mut pos = 56;
        while pos < end {
            ensure!(pos + 32 <= end, "Truncated SEGB v1 header");
            let size = u32_at(b, pos)? as usize;
            let state = u32_at(b, pos + 4)?;
            let next = pos
                .checked_add(32)
                .and_then(|p| p.checked_add(size))
                .context("SEGB size overflow")?;
            ensure!(next <= end, "Truncated SEGB v1 payload");
            let data = &b[pos + 32..next];
            match state {
                1 => emit(Record {
                    offset: pos,
                    timestamp: time(b, pos + 8)?,
                    end_timestamp: Some(time(b, pos + 16)?),
                    crc_valid: crc32fast::hash(data) == u32_at(b, pos + 24)?,
                    data,
                })?,
                0 | 3 | 4 => {}
                _ => anyhow::bail!("Unsupported SEGB record state {state}"),
            }
            pos = (next + 7) & !7;
        }
    } else if b.starts_with(b"SEGB") {
        ensure!(b.len() >= 32, "Truncated SEGB v2 header");
        let count = u32_at(b, 4)? as usize;
        let trailer = b
            .len()
            .checked_sub(count.checked_mul(16).context("SEGB count overflow")?)
            .context("Invalid SEGB trailer size")?;
        ensure!(trailer >= 32, "Invalid SEGB v2 trailer boundary");
        let mut entries = Vec::with_capacity(count);
        for i in 0..count {
            let p = trailer + i * 16;
            entries.push((u32_at(b, p)? as usize, u32_at(b, p + 4)?, p + 8));
        }
        entries.sort_unstable_by_key(|e| e.0);
        let mut pos = 32;
        for (relative_end, state, timestamp_pos) in entries {
            if state == 4 {
                continue;
            }
            let next = relative_end.checked_add(32).context("SEGB end overflow")?;
            ensure!(
                next >= pos + 8 && next <= trailer,
                "Invalid SEGB v2 record boundary"
            );
            let data = &b[pos + 8..next];
            match state {
                1 => emit(Record {
                    offset: pos,
                    timestamp: time(b, timestamp_pos)?,
                    end_timestamp: None,
                    crc_valid: crc32fast::hash(data) == u32_at(b, pos)?,
                    data,
                })?,
                0 | 3 => {}
                _ => anyhow::bail!("Unsupported SEGB record state {state}"),
            }
            pos = (next + 3) & !3;
        }
    } else {
        anyhow::bail!("Unsupported Biome segment format (expected SEGB v1 or v2)");
    }
    Ok(())
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub fn v2(rows: &[(u32, f64, &[u8])]) -> Vec<u8> {
        let mut b = vec![0; 32];
        b[..4].copy_from_slice(b"SEGB");
        b[4..8].copy_from_slice(&(rows.len() as u32).to_le_bytes());
        let mut metadata = Vec::new();
        for (state, time, data) in rows {
            b.extend(crc32fast::hash(data).to_le_bytes());
            b.extend(0u32.to_le_bytes());
            b.extend(*data);
            metadata.extend(((b.len() - 32) as u32).to_le_bytes());
            metadata.extend(state.to_le_bytes());
            metadata.extend(time.to_le_bytes());
            while b.len() % 4 != 0 {
                b.push(0);
            }
        }
        b.extend(metadata);
        b
    }
    #[test]
    fn v2_skips_deleted_and_checks_crc_and_bounds() {
        let mut b = v2(&[(1, 0.5, b"abc"), (3, 1.0, b"gone"), (1, 2.0, b"xyz")]);
        let mut rows = Vec::new();
        parse(&b, |r| {
            rows.push((r.offset, r.timestamp, r.crc_valid, r.data.to_vec()));
            Ok(())
        })
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].3, b"abc");
        assert!(rows[0].2);
        assert_eq!(rows[0].1, apple_time(Some(0.5)).unwrap().unwrap());
        b[40] ^= 1;
        parse(&b, |r| {
            if r.offset == 32 {
                assert!(!r.crc_valid);
            }
            Ok(())
        })
        .unwrap();
        b[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse(&b, |_| Ok(())).is_err());
    }
    #[test]
    fn v1_alignment_and_timestamps() {
        let data = b"hi";
        let mut b = vec![0; 56];
        b[52..56].copy_from_slice(b"SEGB");
        b.extend((data.len() as u32).to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(0.25f64.to_le_bytes());
        b.extend(1.25f64.to_le_bytes());
        b.extend(crc32fast::hash(data).to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(data);
        let end = b.len() as u32;
        b[..4].copy_from_slice(&end.to_le_bytes());
        parse(&b, |r| {
            assert_eq!(r.data, data);
            assert!(r.crc_valid);
            assert_eq!(r.end_timestamp, apple_time(Some(1.25)).unwrap());
            Ok(())
        })
        .unwrap();
        b.pop();
        assert!(parse(&b, |_| Ok(())).is_err());
        assert!(parse(b"unknown", |_| Ok(())).is_err());
    }
}
