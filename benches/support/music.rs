#[cfg(feature = "adl")]
use std::sync::OnceLock as AdlOnceLock;
#[cfg(feature = "midi")]
use std::sync::OnceLock as MidOnceLock;
#[cfg(feature = "xmi")]
use std::sync::OnceLock as XmiOnceLock;

#[cfg(feature = "midi")]
pub(crate) fn mid_bytes() -> &'static [u8] {
    static FIXTURE: MidOnceLock<Vec<u8>> = MidOnceLock::new();
    FIXTURE.get_or_init(build_mid_bytes).as_slice()
}

#[cfg(feature = "adl")]
pub(crate) fn adl_bytes() -> &'static [u8] {
    static FIXTURE: AdlOnceLock<Vec<u8>> = AdlOnceLock::new();
    FIXTURE.get_or_init(build_adl_bytes).as_slice()
}

#[cfg(feature = "xmi")]
pub(crate) fn xmi_bytes() -> &'static [u8] {
    static FIXTURE: XmiOnceLock<Vec<u8>> = XmiOnceLock::new();
    FIXTURE.get_or_init(build_xmi_bytes).as_slice()
}

#[cfg(feature = "midi")]
fn build_mid_bytes() -> Vec<u8> {
    let mut buf = Vec::new();
    let track_data: Vec<u8> = vec![
        0x00, 0x90, 0x3C, 0x64, 0x30, 0x80, 0x3C, 0x00, 0x00, 0x90, 0x40, 0x60, 0x30, 0x80, 0x40,
        0x00, 0x00, 0xFF, 0x2F, 0x00,
    ];

    buf.extend_from_slice(b"MThd");
    buf.extend_from_slice(&0x0000_0006u32.to_be_bytes());
    buf.extend_from_slice(&0x0000u16.to_be_bytes());
    buf.extend_from_slice(&0x0001u16.to_be_bytes());
    buf.extend_from_slice(&0x0060u16.to_be_bytes());
    buf.extend_from_slice(b"MTrk");
    buf.extend_from_slice(&(track_data.len() as u32).to_be_bytes());
    buf.extend_from_slice(&track_data);
    buf
}

#[cfg(feature = "adl")]
fn build_adl_bytes() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&[
        0x21, 0x31, 0x4F, 0x00, 0xF2, 0xF2, 0x60, 0x60, 0x00, 0x00, 0x06,
    ]);
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(&[0x20, 0x21, 0x40, 0x3F, 0xA0, 0x98]);
    data
}

#[cfg(feature = "xmi")]
fn build_xmi_bytes() -> Vec<u8> {
    let mut data = Vec::new();
    let evnt_data: Vec<u8> = vec![0x90, 60, 100, 60, 0xFF, 0x2F, 0x00];
    let evnt_padded = if evnt_data.len() % 2 == 1 {
        evnt_data.len() + 1
    } else {
        evnt_data.len()
    };
    let form_body_size = 4 + 8 + evnt_data.len();
    let form_body_size_padded = 4 + 8 + evnt_padded;
    let cat_body_size = 4 + 8 + form_body_size_padded;

    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&14u32.to_be_bytes());
    data.extend_from_slice(b"XDIR");
    data.extend_from_slice(b"INFO");
    data.extend_from_slice(&2u32.to_be_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());

    data.extend_from_slice(b"CAT ");
    data.extend_from_slice(&(cat_body_size as u32).to_be_bytes());
    data.extend_from_slice(b"XMID");
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&(form_body_size as u32).to_be_bytes());
    data.extend_from_slice(b"XMID");
    data.extend_from_slice(b"EVNT");
    data.extend_from_slice(&(evnt_data.len() as u32).to_be_bytes());
    data.extend_from_slice(&evnt_data);
    if evnt_data.len() % 2 == 1 {
        data.push(0);
    }
    data
}
