use cnc_formats::mix::{self, MixCrc};

use std::sync::OnceLock;

pub(crate) struct MixFixture {
    pub bytes: Vec<u8>,
    pub query_crc: MixCrc,
}

pub(crate) struct NamedArchiveFixture {
    pub bytes: Vec<u8>,
    pub query_index: usize,
}

pub(crate) fn mix_fixture() -> &'static MixFixture {
    static FIXTURE: OnceLock<MixFixture> = OnceLock::new();
    FIXTURE.get_or_init(build_mix_fixture)
}

pub(crate) fn big_fixture() -> &'static NamedArchiveFixture {
    static FIXTURE: OnceLock<NamedArchiveFixture> = OnceLock::new();
    FIXTURE.get_or_init(build_big_fixture)
}

#[cfg(feature = "meg")]
pub(crate) fn meg_fixture() -> &'static NamedArchiveFixture {
    static FIXTURE: OnceLock<NamedArchiveFixture> = OnceLock::new();
    FIXTURE.get_or_init(build_meg_fixture)
}

fn build_mix_fixture() -> MixFixture {
    const TARGET: &str = "TARGET.BIN";
    let files = build_archive_files(TARGET, "FILE", 63, 2048, 4096);
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_slice()))
        .collect();

    MixFixture {
        bytes: build_mix(&refs),
        query_crc: mix::crc(TARGET),
    }
}

fn build_big_fixture() -> NamedArchiveFixture {
    const TARGET: &str = "DATA/ART/TARGET.SHP";
    let files = build_archive_files(TARGET, "DATA/ART/FILE", 63, 2048, 4096);
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_slice()))
        .collect();

    NamedArchiveFixture {
        bytes: build_big(b"BIGF", &refs),
        query_index: refs.len().saturating_sub(1),
    }
}

#[cfg(feature = "meg")]
fn build_meg_fixture() -> NamedArchiveFixture {
    const TARGET: &str = "DATA/ART/TARGET.WSA";
    let files = build_archive_files(TARGET, "DATA/ART/FILE", 63, 2048, 4096);
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, data)| (name.as_str(), data.as_slice()))
        .collect();

    NamedArchiveFixture {
        bytes: build_meg(&refs),
        query_index: refs.len().saturating_sub(1),
    }
}

fn build_archive_files(
    target_name: &'static str,
    prefix: &str,
    count: usize,
    payload_len: usize,
    target_len: usize,
) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::with_capacity(count.saturating_add(1));
    for index in 0..count {
        files.push((
            format!("{prefix}{index:03}.BIN"),
            build_payload(index as u8, payload_len),
        ));
    }
    files.push((target_name.to_owned(), build_payload(0xF3, target_len)));
    files
}

fn build_payload(seed: u8, len: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(len);
    for index in 0..len {
        data.push(seed.wrapping_add((index % 251) as u8));
    }
    data
}

fn build_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries: Vec<(MixCrc, &[u8])> = files
        .iter()
        .map(|(name, data)| (mix::crc(name), *data))
        .collect();
    entries.sort_by_key(|(crc, _)| crc.to_raw() as i32);

    let count = entries.len() as u16;
    let mut offsets = Vec::with_capacity(entries.len());
    let mut current = 0u32;
    for (_, data) in &entries {
        offsets.push(current);
        current = current.saturating_add(data.len() as u32);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&current.to_le_bytes());
    for (index, (crc, data)) in entries.iter().enumerate() {
        out.extend_from_slice(&crc.to_raw().to_le_bytes());
        out.extend_from_slice(&offsets[index].to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    }
    for (_, data) in &entries {
        out.extend_from_slice(data);
    }
    out
}

fn build_big(magic: &[u8; 4], files: &[(&str, &[u8])]) -> Vec<u8> {
    let table_size: usize = files.iter().map(|(name, _)| 8 + name.len() + 1).sum();
    let data_start = 16 + table_size;
    let archive_size = data_start + files.iter().map(|(_, data)| data.len()).sum::<usize>();

    let mut out = Vec::with_capacity(archive_size);
    out.extend_from_slice(magic);
    out.extend_from_slice(&(archive_size as u32).to_le_bytes());
    out.extend_from_slice(&(files.len() as u32).to_be_bytes());
    out.extend_from_slice(&(data_start as u32).to_be_bytes());

    let mut offset = data_start as u32;
    for (name, data) in files {
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        offset = offset.saturating_add(data.len() as u32);
    }

    for (_, data) in files {
        out.extend_from_slice(data);
    }
    out
}

#[cfg(feature = "meg")]
fn build_meg(files: &[(&str, &[u8])]) -> Vec<u8> {
    let count = files.len() as u32;
    let mut buf = Vec::new();

    buf.extend_from_slice(&count.to_le_bytes());
    buf.extend_from_slice(&count.to_le_bytes());

    for (name, _) in files {
        let name_len = name.len() as u16;
        buf.extend_from_slice(&name_len.to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
    }

    let records_total = files.len() * 20;
    let data_start = buf.len() + records_total;

    let mut offset = data_start;
    let mut offsets = Vec::with_capacity(files.len());
    for (_, data) in files {
        offsets.push(offset as u32);
        offset += data.len();
    }

    for (index, (_, data)) in files.iter().enumerate() {
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&(index as u32).to_le_bytes());
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&offsets[index].to_le_bytes());
        buf.extend_from_slice(&(index as u32).to_le_bytes());
    }

    for (_, data) in files {
        buf.extend_from_slice(data);
    }
    buf
}
