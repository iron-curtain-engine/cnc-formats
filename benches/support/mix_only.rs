use cnc_formats::mix::{self, MixCrc};

use std::sync::OnceLock;

pub(crate) struct MixFixture {
    pub bytes: Vec<u8>,
    pub query_crc: MixCrc,
}

pub(crate) fn mix_fixture() -> &'static MixFixture {
    static FIXTURE: OnceLock<MixFixture> = OnceLock::new();
    FIXTURE.get_or_init(build_mix_fixture)
}

fn build_mix_fixture() -> MixFixture {
    const TARGET: &str = "TARGET.BIN";
    let files = [
        ("FILE000.BIN", vec![0x10; 1024]),
        ("FILE001.BIN", vec![0x20; 1024]),
        (TARGET, vec![0x30; 2048]),
    ];
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, data)| (*name, data.as_slice()))
        .collect();

    MixFixture {
        bytes: build_mix(&refs),
        query_crc: mix::crc(TARGET),
    }
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
