use cnc_formats::lut::LUT_ENTRY_COUNT;

use std::sync::OnceLock;

pub(crate) fn pal_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_pal_bytes).as_slice()
}

pub(crate) fn fnt_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(|| build_fnt(12, 8, 192)).as_slice()
}

pub(crate) fn eng_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_eng_bytes).as_slice()
}

pub(crate) fn dip_segmented_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_segmented_dip_bytes).as_slice()
}

pub(crate) fn vqp_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(|| build_vqp(2)).as_slice()
}

pub(crate) fn lut_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_lut_bytes).as_slice()
}

pub(crate) fn ini_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_ini_bytes).as_slice()
}

#[cfg(feature = "miniyaml")]
pub(crate) fn miniyaml_bytes() -> &'static [u8] {
    static FIXTURE: OnceLock<Vec<u8>> = OnceLock::new();
    FIXTURE.get_or_init(build_miniyaml_bytes).as_slice()
}

fn build_pal_bytes() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(256usize.saturating_mul(3));
    for index in 0..256usize {
        bytes.push((index % 64) as u8);
        bytes.push(((index * 3) % 64) as u8);
        bytes.push(((index * 5) % 64) as u8);
    }
    bytes
}

fn build_eng_bytes() -> Vec<u8> {
    let mut strings = Vec::with_capacity(32);
    for index in 0..32usize {
        strings.push(format!("Benchmark string {index}"));
    }

    let table_len = strings.len().saturating_mul(2);
    let mut out = vec![0u8; table_len];
    let mut offset = table_len as u16;
    for (index, string) in strings.iter().enumerate() {
        out[index * 2..index * 2 + 2].copy_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(string.as_bytes());
        out.push(0);
        offset = offset.saturating_add(string.len() as u16).saturating_add(1);
    }
    out
}

fn build_segmented_dip_bytes() -> Vec<u8> {
    let sections = [
        [0x00u8, 0x00, 0x3C, 0x3C].as_slice(),
        [0x01u8, 0x80, 0x00, 0x00].as_slice(),
        [0x82u8, 0x10, 0x20, 0x30].as_slice(),
    ];
    let header_size = 4usize.saturating_add(sections.len().saturating_mul(4));
    let mut out = Vec::new();
    out.extend_from_slice(&(sections.len() as u16).to_le_bytes());
    out.extend_from_slice(&(header_size as u16).to_le_bytes());

    let mut end = header_size;
    for section in &sections {
        end = end.saturating_add(section.len());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(end as u16).to_le_bytes());
    }

    for section in &sections {
        out.extend_from_slice(section);
    }
    out.extend_from_slice(&[0x0B, 0x80]);
    out
}

fn build_vqp(table_count: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&table_count.to_le_bytes());
    for table in 0..table_count {
        for row in 0u16..256 {
            for col in 0u16..=row {
                let value = ((table as u16).wrapping_add(row).wrapping_add(col) & 0xFF) as u8;
                out.push(value);
            }
        }
    }
    out
}

fn build_lut_bytes() -> Vec<u8> {
    let mut out = Vec::with_capacity(LUT_ENTRY_COUNT.saturating_mul(3));
    for index in 0..LUT_ENTRY_COUNT {
        out.push((index % 64) as u8);
        out.push(((index / 64) % 64) as u8);
        out.push(((index / 256) % 16) as u8);
    }
    out
}

fn build_ini_bytes() -> Vec<u8> {
    let mut text = String::new();
    for section in 0..16usize {
        text.push('[');
        text.push_str("Section");
        text.push_str(&section.to_string());
        text.push_str("]\n");
        for key in 0..16usize {
            text.push_str("Key");
            text.push_str(&key.to_string());
            text.push('=');
            text.push_str("Value");
            text.push_str(&section.to_string());
            text.push('_');
            text.push_str(&key.to_string());
            text.push('\n');
        }
        text.push('\n');
    }
    text.into_bytes()
}

#[cfg(feature = "miniyaml")]
fn build_miniyaml_bytes() -> Vec<u8> {
    let mut text = String::new();
    for index in 0..32usize {
        text.push_str("Node");
        text.push_str(&index.to_string());
        text.push_str(":\n");
        text.push_str("  Key: Value");
        text.push_str(&index.to_string());
        text.push('\n');
        text.push_str("  Cost: ");
        text.push_str(&(index * 10).to_string());
        text.push('\n');
    }
    text.into_bytes()
}

fn build_fnt(max_height: u8, glyph_w: u8, num_chars: u16) -> Vec<u8> {
    let num_chars_usize = usize::from(num_chars);
    let bytes_per_row = usize::from(glyph_w).div_ceil(2);
    let glyph_size = bytes_per_row.saturating_mul(max_height as usize);

    let offset_table_start = 20usize;
    let offset_table_size = num_chars_usize.saturating_mul(2);
    let width_table_start = offset_table_start.saturating_add(offset_table_size);
    let width_table_size = num_chars_usize;
    let data_area_start = width_table_start.saturating_add(width_table_size);
    let height_table_start = data_area_start.saturating_add(glyph_size);
    let total = height_table_start.saturating_add(num_chars_usize.saturating_mul(2));

    let mut buf = vec![0u8; total];
    write_u16_le(&mut buf, 0, total as u16);
    buf[2] = 0;
    buf[3] = 5;
    write_u16_le(&mut buf, 4, 0x0010);
    write_u16_le(&mut buf, 6, offset_table_start as u16);
    write_u16_le(&mut buf, 8, width_table_start as u16);
    write_u16_le(&mut buf, 10, data_area_start as u16);
    write_u16_le(&mut buf, 12, height_table_start as u16);
    write_u16_le(&mut buf, 14, 0x1012);
    buf[16] = 0;
    buf[17] = num_chars.saturating_sub(1) as u8;
    buf[18] = max_height;
    buf[19] = glyph_w;

    if num_chars_usize > 0x41 {
        buf[width_table_start + 0x41] = glyph_w;
        write_u16_le(
            &mut buf,
            offset_table_start + 0x41 * 2,
            data_area_start as u16,
        );
        write_u16_le(
            &mut buf,
            height_table_start + 0x41 * 2,
            (u16::from(max_height)) << 8,
        );
    }

    for byte in &mut buf[data_area_start..data_area_start + glyph_size] {
        *byte = 0x12;
    }

    buf
}

fn write_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
